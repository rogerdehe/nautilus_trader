// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! MEXC spot v3 REST HTTP client.

use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::{Arc, LazyLock, Mutex},
};

use ahash::AHashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::AccountBalance,
};
use nautilus_network::{http::HttpClient, ratelimiter::quota::Quota};
use reqwest::Method;
use serde::de::DeserializeOwned;
use ustr::Ustr;

use super::{
    error::{MexcErrorResponse, MexcHttpError},
    models::{
        MexcAccount, MexcExchangeInfo, MexcListenKey, MexcMyTrade, MexcOrder, MexcTrade,
    },
    parse::{
        is_spot_enabled, parse_account_balances, parse_fill_report, parse_order_status_report,
        parse_spot_instrument, parse_trade_tick,
    },
};
use crate::common::{
    consts::{
        EP_ACCOUNT, EP_EXCHANGE_INFO, EP_LISTEN_KEY, EP_MY_TRADES, EP_OPEN_ORDERS, EP_ORDER,
        EP_TIME, EP_TRADES, MEXC_BROKER_SOURCE, MEXC_HTTP_URL, MEXC_RATE_LIMIT_PER_SEC,
        MEXC_RECV_WINDOW_MS,
    },
    credential::Credential,
};

type Result<T> = std::result::Result<T, MexcHttpError>;

/// Global default rate-limit quota for MEXC (CCXT `rateLimit` = 50ms => 20/s).
pub static MEXC_REST_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(MEXC_RATE_LIMIT_PER_SEC).expect("non-zero")).expect("valid")
});

/// Cached instrument precision (price_precision, size_precision).
#[derive(Copy, Clone, Debug)]
struct Precisions {
    price: u8,
    size: u8,
}

/// The inner (shared) MEXC HTTP client state.
#[derive(Debug)]
struct MexcHttpInner {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
    recv_window_ms: u64,
    /// Instrument precision cache keyed by market id (e.g. `BTCUSDT`).
    precisions: Mutex<AHashMap<Ustr, Precisions>>,
}

/// MEXC spot v3 REST HTTP client (cheaply cloneable).
#[derive(Debug, Clone)]
pub struct MexcHttpClient {
    inner: Arc<MexcHttpInner>,
}

impl MexcHttpClient {
    /// Creates a new **public** (unauthenticated) MEXC HTTP client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be constructed.
    pub fn new(base_url: Option<String>, timeout_secs: Option<u64>) -> Result<Self> {
        Self::build(base_url, timeout_secs, None)
    }

    /// Creates a new MEXC HTTP client configured with API credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be constructed.
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> Result<Self> {
        Self::build(base_url, timeout_secs, Some(Credential::new(api_key, api_secret)))
    }

    fn build(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        credential: Option<Credential>,
    ) -> Result<Self> {
        let client = HttpClient::new(
            HashMap::new(),
            vec![],
            vec![],
            Some(*MEXC_REST_QUOTA),
            timeout_secs,
            None,
        )
        .map_err(|e| MexcHttpError::ValidationError(format!("Failed to create HTTP client: {e}")))?;

        Ok(Self {
            inner: Arc::new(MexcHttpInner {
                base_url: base_url.unwrap_or_else(|| MEXC_HTTP_URL.to_string()),
                client,
                credential,
                recv_window_ms: MEXC_RECV_WINDOW_MS,
                precisions: Mutex::new(AHashMap::new()),
            }),
        })
    }

    /// Returns `true` when the client has credentials configured.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.inner.credential.is_some()
    }

    fn now_millis() -> u64 {
        chrono::Utc::now().timestamp_millis().max(0) as u64
    }

    /// Sends a public (unsigned) request and deserializes the JSON body into `T`.
    async fn send_public<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<T> {
        let query = serde_urlencoded::to_string(params)
            .map_err(|e| MexcHttpError::JsonError(format!("param encode: {e}")))?;
        let url = if query.is_empty() {
            format!("{}{path}", self.inner.base_url)
        } else {
            format!("{}{path}?{query}", self.inner.base_url)
        };
        self.dispatch(method, url, None).await
    }

    /// Sends a signed (private) request and deserializes the JSON body into `T`.
    ///
    /// Signing follows CCXT `mexc.sign()` for the spot section: the signature is a
    /// hex HMAC-SHA256 over the exact URL-encoded query (business params, then
    /// `timestamp`, then `recvWindow`), and `&signature=<sig>` is appended verbatim.
    async fn send_signed<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<T> {
        let credential = self
            .inner
            .credential
            .as_ref()
            .ok_or(MexcHttpError::MissingCredentials)?;

        let mut all: Vec<(&str, String)> = params.to_vec();
        let ts = Self::now_millis().to_string();
        all.push(("timestamp", ts));
        all.push(("recvWindow", self.inner.recv_window_ms.to_string()));

        let query = serde_urlencoded::to_string(&all)
            .map_err(|e| MexcHttpError::JsonError(format!("param encode: {e}")))?;
        let signature = credential.sign(&query);
        let url = format!("{}{path}?{query}&signature={signature}", self.inner.base_url);

        let mut headers = HashMap::new();
        headers.insert("X-MEXC-APIKEY".to_string(), credential.api_key().to_string());
        headers.insert("source".to_string(), MEXC_BROKER_SOURCE.to_string());
        // CCXT sets Content-Type on POST/PUT/DELETE (body is empty; params live in the query string).
        if matches!(method, Method::POST | Method::PUT | Method::DELETE) {
            headers.insert("Content-Type".to_string(), "application/json".to_string());
        }

        self.dispatch(method, url, Some(headers)).await
    }

    async fn dispatch<T: DeserializeOwned>(
        &self,
        method: Method,
        url: String,
        headers: Option<HashMap<String, String>>,
    ) -> Result<T> {
        let resp = self
            .inner
            .client
            .request(method, url, None, headers, None, None, None)
            .await
            .map_err(|e| MexcHttpError::NetworkError(e.to_string()))?;

        if resp.status.is_success() {
            serde_json::from_slice::<T>(&resp.body).map_err(|e| {
                MexcHttpError::JsonError(format!(
                    "deserialize: {e}; body={}",
                    String::from_utf8_lossy(&resp.body)
                ))
            })
        } else {
            // Try to parse the MEXC error envelope; otherwise fall back to raw body.
            if let Ok(err) = serde_json::from_slice::<MexcErrorResponse>(&resp.body) {
                Err(MexcHttpError::MexcError {
                    code: err.code,
                    message: err.msg.unwrap_or_default(),
                })
            } else {
                Err(MexcHttpError::UnexpectedStatus {
                    status: resp.status.as_u16(),
                    body: String::from_utf8_lossy(&resp.body).to_string(),
                })
            }
        }
    }

    fn cache_precisions(&self, instruments: &[InstrumentAny]) {
        let mut cache = self.inner.precisions.lock().expect("poisoned");
        for inst in instruments {
            cache.insert(
                Ustr::from(inst.id().symbol.as_str()),
                Precisions {
                    price: inst.price_precision(),
                    size: inst.size_precision(),
                },
            );
        }
    }

    fn precisions_for(&self, instrument_id: &InstrumentId) -> Precisions {
        self.inner
            .precisions
            .lock()
            .expect("poisoned")
            .get(&Ustr::from(instrument_id.symbol.as_str()))
            .copied()
            .unwrap_or(Precisions { price: 8, size: 8 })
    }

    // --- Public endpoints ------------------------------------------------------------------------

    /// Fetches server time (ms). Useful as a connectivity check.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn request_server_time(&self) -> Result<i64> {
        #[derive(serde::Deserialize)]
        struct ServerTime {
            #[serde(rename = "serverTime")]
            server_time: i64,
        }
        let t: ServerTime = self.send_public(Method::GET, EP_TIME, &[]).await?;
        Ok(t.server_time)
    }

    /// Fetches all enabled spot instruments and caches their precisions.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn request_instruments(&self, ts_init: UnixNanos) -> Result<Vec<InstrumentAny>> {
        let info: MexcExchangeInfo = self.send_public(Method::GET, EP_EXCHANGE_INFO, &[]).await?;
        let mut out = Vec::with_capacity(info.symbols.len());
        for symbol in &info.symbols {
            if !is_spot_enabled(symbol) {
                continue;
            }
            match parse_spot_instrument(symbol, ts_init) {
                Ok(inst) => out.push(inst),
                Err(e) => log::warn!("Skipping MEXC symbol {}: {e}", symbol.symbol),
            }
        }
        self.cache_precisions(&out);
        Ok(out)
    }

    /// Fetches recent trades for `instrument_id` as [`TradeTick`]s.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn request_trades(
        &self,
        instrument_id: InstrumentId,
        limit: Option<u32>,
        ts_init: UnixNanos,
    ) -> Result<Vec<nautilus_model::data::TradeTick>> {
        let mut params = vec![("symbol", instrument_id.symbol.as_str().to_string())];
        if let Some(l) = limit {
            params.push(("limit", l.to_string()));
        }
        let trades: Vec<MexcTrade> = self.send_public(Method::GET, EP_TRADES, &params).await?;
        let p = self.precisions_for(&instrument_id);
        let mut out = Vec::with_capacity(trades.len());
        for t in &trades {
            match parse_trade_tick(t, instrument_id, p.price, p.size, ts_init) {
                Ok(tick) => out.push(tick),
                Err(e) => log::warn!("Skipping MEXC trade for {instrument_id}: {e}"),
            }
        }
        Ok(out)
    }

    // --- Private endpoints -----------------------------------------------------------------------

    /// Fetches spot account balances as Nautilus [`AccountBalance`]s.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn request_account_balances(&self) -> Result<Vec<AccountBalance>> {
        let account: MexcAccount = self.send_signed(Method::GET, EP_ACCOUNT, &[]).await?;
        Ok(parse_account_balances(&account.balances))
    }

    /// Fetches open orders (optionally filtered by `instrument_id`) as
    /// [`OrderStatusReport`]s.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn request_order_status_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        ts_init: UnixNanos,
    ) -> Result<Vec<OrderStatusReport>> {
        let mut params = Vec::new();
        if let Some(id) = instrument_id {
            params.push(("symbol", id.symbol.as_str().to_string()));
        }
        let orders: Vec<MexcOrder> = self.send_signed(Method::GET, EP_OPEN_ORDERS, &params).await?;
        let mut out = Vec::with_capacity(orders.len());
        for o in &orders {
            let inst_id = crate::common::parse::instrument_id_from_symbol(&o.symbol);
            let p = self.precisions_for(&inst_id);
            match parse_order_status_report(o, account_id, p.price, p.size, ts_init) {
                Ok(r) => out.push(r),
                Err(e) => log::warn!("Skipping MEXC order {}: {e}", o.order_id),
            }
        }
        Ok(out)
    }

    /// Fetches account trades (fills) for `instrument_id` as [`FillReport`]s.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        ts_init: UnixNanos,
    ) -> Result<Vec<FillReport>> {
        let params = vec![("symbol", instrument_id.symbol.as_str().to_string())];
        let trades: Vec<MexcMyTrade> = self.send_signed(Method::GET, EP_MY_TRADES, &params).await?;
        let p = self.precisions_for(&instrument_id);
        let mut out = Vec::with_capacity(trades.len());
        for t in &trades {
            match parse_fill_report(t, account_id, p.price, p.size, ts_init) {
                Ok(r) => out.push(r),
                Err(e) => log::warn!("Skipping MEXC fill {}: {e}", t.id),
            }
        }
        Ok(out)
    }

    /// Submits a new spot order and returns the raw [`MexcOrder`] acknowledgement.
    ///
    /// `params` are the fully-formed MEXC order fields (symbol, side, type,
    /// quantity/quoteOrderQty, price, newClientOrderId, ...).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn submit_order(&self, params: &[(&str, String)]) -> Result<MexcOrder> {
        self.send_signed(Method::POST, EP_ORDER, params).await
    }

    /// Cancels a spot order by `symbol` and `orderId` (or `origClientOrderId`).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn cancel_order(&self, params: &[(&str, String)]) -> Result<MexcOrder> {
        self.send_signed(Method::DELETE, EP_ORDER, params).await
    }

    /// Creates a user data stream listen key for the private WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn create_listen_key(&self) -> Result<String> {
        let key: MexcListenKey = self.send_signed(Method::POST, EP_LISTEN_KEY, &[]).await?;
        Ok(key.listen_key)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_public_client_builds() {
        let client = MexcHttpClient::new(None, Some(30)).unwrap();
        assert!(!client.has_credentials());
    }

    #[rstest]
    fn test_credentialed_client_builds() {
        let client =
            MexcHttpClient::with_credentials("k".to_string(), "s".to_string(), None, Some(30))
                .unwrap();
        assert!(client.has_credentials());
    }

    #[rstest]
    fn test_default_precisions_fallback() {
        let client = MexcHttpClient::new(None, None).unwrap();
        let p = client.precisions_for(&InstrumentId::from("BTCUSDT.MEXC"));
        assert_eq!(p.price, 8);
        assert_eq!(p.size, 8);
    }
}
