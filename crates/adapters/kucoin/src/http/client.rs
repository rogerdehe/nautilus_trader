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

//! HTTP/REST client for the KuCoin spot API (<https://www.kucoin.com/docs-new>).
//!
//! Signing, endpoints and envelopes are implemented first-hand from
//! `ccxt/python/ccxt/kucoin.py`.

use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{Arc, Mutex},
};

use ahash::AHashMap;
use nautilus_core::{
    UnixNanos, consts::NAUTILUS_USER_AGENT, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::TradeTick,
    events::AccountState,
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
};
use nautilus_network::{
    http::{HttpClient, Method, StatusCode, USER_AGENT},
    ratelimiter::quota::Quota,
};
use serde::de::DeserializeOwned;

use super::{
    error::KuCoinHttpError,
    models::{
        KuCoinAccount, KuCoinBulletToken, KuCoinFill, KuCoinOrder, KuCoinOrderBook,
        KuCoinOrderCreated, KuCoinResponse, KuCoinSymbol, KuCoinTrade,
    },
    parse::{
        parse_account_state, parse_fill_report, parse_order_status_report, parse_spot_instrument,
        parse_trade_tick,
    },
};
use crate::common::{
    consts::{
        EP_ACCOUNTS, EP_BULLET_PRIVATE, EP_BULLET_PUBLIC, EP_FILLS, EP_ORDERBOOK_L2_100,
        EP_ORDERS, EP_SYMBOLS, EP_TRADE_HISTORIES, KUCOIN_HTTP_URL, KUCOIN_RATE_LIMIT_MS,
        KUCOIN_SUCCESS_CODE,
    },
    credential::{Credential, KC_API_KEY_VERSION},
};

/// Default request timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 60;

fn default_quota() -> Quota {
    // ~10 requests/sec baseline derived from CCXT's `rateLimit` (KUCOIN_RATE_LIMIT_MS ms).
    let per_second = (1_000 / KUCOIN_RATE_LIMIT_MS).max(1) as u32;
    Quota::per_second(std::num::NonZeroU32::new(per_second).expect("non-zero quota"))
        .expect("valid quota")
}

/// Raw (inner) KuCoin HTTP client holding the network client + credentials.
#[derive(Debug)]
struct KuCoinRawHttpClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
}

impl KuCoinRawHttpClient {
    fn new(base_url: Option<String>, credential: Option<Credential>) -> Result<Self, KuCoinHttpError> {
        let headers = HashMap::from([(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())]);
        let client = HttpClient::new(
            headers,
            vec![],
            vec![],
            Some(default_quota()),
            Some(DEFAULT_TIMEOUT_SECS),
            None,
        )
        .map_err(|e| KuCoinHttpError::ValidationError(format!("Failed to build HTTP client: {e}")))?;
        Ok(Self {
            base_url: base_url.unwrap_or_else(|| KUCOIN_HTTP_URL.to_string()),
            client,
            credential,
        })
    }

    /// Builds the KuCoin auth headers for a signed request.
    ///
    /// `endpoint` must be the full path INCLUDING `/api/v1/...` and any `?query`.
    fn sign_headers(
        &self,
        method: &Method,
        endpoint: &str,
        body: &str,
    ) -> Result<HashMap<String, String>, KuCoinHttpError> {
        let credential = self
            .credential
            .as_ref()
            .ok_or(KuCoinHttpError::MissingCredentials)?;
        let timestamp = get_atomic_clock_realtime().get_time_ms().to_string();
        let signature = credential.sign(&timestamp, method.as_str(), endpoint, body);
        let passphrase = credential.sign_passphrase();

        let mut headers = HashMap::new();
        headers.insert("KC-API-KEY".to_string(), credential.api_key().to_string());
        headers.insert("KC-API-SIGN".to_string(), signature);
        headers.insert("KC-API-TIMESTAMP".to_string(), timestamp);
        headers.insert("KC-API-PASSPHRASE".to_string(), passphrase);
        headers.insert(
            "KC-API-KEY-VERSION".to_string(),
            KC_API_KEY_VERSION.to_string(),
        );
        Ok(headers)
    }

    /// Sends a request and unwraps the KuCoin `{code,data}` envelope into `T`.
    ///
    /// `endpoint` is the full path (`/api/v1/...`); `query` (already URL-encoded, no leading `?`)
    /// is appended to both the signed path and the request URL so they match byte-for-byte.
    async fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        query: Option<&str>,
        body: Option<String>,
        authenticate: bool,
    ) -> Result<T, KuCoinHttpError> {
        let signed_path = match query {
            Some(q) if !q.is_empty() => format!("{endpoint}?{q}"),
            _ => endpoint.to_string(),
        };
        let url = format!("{}{signed_path}", self.base_url);
        let body_str = body.unwrap_or_default();

        let mut headers = if authenticate {
            self.sign_headers(&method, &signed_path, &body_str)?
        } else {
            HashMap::new()
        };
        let body_bytes = if body_str.is_empty() {
            None
        } else {
            headers.insert("Content-Type".to_string(), "application/json".to_string());
            Some(body_str.into_bytes())
        };

        let resp = self
            .client
            .request(method, url, None, Some(headers), body_bytes, None, None)
            .await?;

        if !resp.status.is_success() {
            // Attempt to surface KuCoin's business error even on non-2xx.
            if let Ok(env) = serde_json::from_slice::<KuCoinResponse<serde_json::Value>>(&resp.body)
            {
                return Err(KuCoinHttpError::KuCoinError {
                    error_code: env.code,
                    message: env.msg.unwrap_or_default(),
                });
            }
            return Err(KuCoinHttpError::UnexpectedStatus {
                status: StatusCode::from_u16(resp.status.as_u16())
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                body: String::from_utf8_lossy(&resp.body).to_string(),
            });
        }

        let env: KuCoinResponse<T> = serde_json::from_slice(&resp.body)?;
        if env.code != KUCOIN_SUCCESS_CODE {
            return Err(KuCoinHttpError::KuCoinError {
                error_code: env.code,
                message: env.msg.unwrap_or_default(),
            });
        }
        env.data
            .ok_or_else(|| KuCoinHttpError::JsonError("Missing 'data' in response".to_string()))
    }
}

/// The KuCoin HTTP client (thin `Arc` wrapper with an instrument cache).
#[derive(Clone)]
pub struct KuCoinHttpClient {
    inner: Arc<KuCoinRawHttpClient>,
    instruments: Arc<Mutex<AHashMap<InstrumentId, InstrumentAny>>>,
}

impl Debug for KuCoinHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KuCoinHttpClient))
            .field("base_url", &self.inner.base_url)
            .field("has_credentials", &self.inner.credential.is_some())
            .finish()
    }
}

impl KuCoinHttpClient {
    /// Creates a new public (unauthenticated) KuCoin HTTP client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be constructed.
    pub fn new(base_url: Option<String>) -> Result<Self, KuCoinHttpError> {
        Ok(Self {
            inner: Arc::new(KuCoinRawHttpClient::new(base_url, None)?),
            instruments: Arc::new(Mutex::new(AHashMap::new())),
        })
    }

    /// Creates a new KuCoin HTTP client with credentials for signed requests.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be constructed.
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        api_passphrase: String,
        base_url: Option<String>,
    ) -> Result<Self, KuCoinHttpError> {
        let credential = Credential::new(api_key, api_secret, api_passphrase);
        Ok(Self {
            inner: Arc::new(KuCoinRawHttpClient::new(base_url, Some(credential))?),
            instruments: Arc::new(Mutex::new(AHashMap::new())),
        })
    }

    /// Returns `true` if the client holds credentials.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.inner.credential.is_some()
    }

    fn ts_init(&self) -> UnixNanos {
        get_atomic_clock_realtime().get_time_ns()
    }

    /// Caches instruments (replacing any with the same id).
    pub fn cache_instruments(&self, instruments: &[InstrumentAny]) {
        let mut guard = self.instruments.lock().expect("instruments lock poisoned");
        for inst in instruments {
            guard.insert(inst.id(), inst.clone());
        }
    }

    fn precisions(&self, instrument_id: &InstrumentId) -> (u8, u8) {
        self.instruments
            .lock()
            .expect("instruments lock poisoned")
            .get(instrument_id)
            .map_or((8, 8), |i| (i.price_precision(), i.size_precision()))
    }

    // -------------------------------------------------------------------------------------------
    // Public endpoints
    // -------------------------------------------------------------------------------------------

    /// Requests all spot instruments and caches them.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or a definition cannot be parsed.
    pub async fn request_instruments(&self) -> Result<Vec<InstrumentAny>, KuCoinHttpError> {
        let symbols: Vec<KuCoinSymbol> = self
            .inner
            .send(Method::GET, EP_SYMBOLS, None, None, false)
            .await?;
        let ts_init = self.ts_init();
        let mut instruments = Vec::with_capacity(symbols.len());
        for def in &symbols {
            if !def.enable_trading {
                continue;
            }
            match parse_spot_instrument(def, ts_init) {
                Ok(inst) => instruments.push(inst),
                Err(e) => log::warn!("Skipping instrument {}: {e}", def.symbol),
            }
        }
        self.cache_instruments(&instruments);
        Ok(instruments)
    }

    /// Requests the full order book snapshot (top 100) for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn request_order_book(
        &self,
        symbol: &str,
    ) -> Result<KuCoinOrderBook, KuCoinHttpError> {
        let query = format!("symbol={symbol}");
        self.inner
            .send(
                Method::GET,
                EP_ORDERBOOK_L2_100,
                Some(&query),
                None,
                false,
            )
            .await
    }

    /// Requests recent public trades for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or a trade cannot be parsed.
    pub async fn request_trades(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<Vec<TradeTick>, KuCoinHttpError> {
        let symbol = instrument_id.symbol.as_str();
        let query = format!("symbol={symbol}");
        let raw: Vec<KuCoinTrade> = self
            .inner
            .send(Method::GET, EP_TRADE_HISTORIES, Some(&query), None, false)
            .await?;
        let (price_precision, size_precision) = self.precisions(&instrument_id);
        let ts_init = self.ts_init();
        let mut trades = Vec::with_capacity(raw.len());
        for t in &raw {
            match parse_trade_tick(t, instrument_id, price_precision, size_precision, ts_init) {
                Ok(tick) => trades.push(tick),
                Err(e) => log::warn!("Skipping trade for {instrument_id}: {e}"),
            }
        }
        Ok(trades)
    }

    /// Requests a public WebSocket bullet token (unauthenticated channels).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn request_bullet_public(&self) -> Result<KuCoinBulletToken, KuCoinHttpError> {
        self.inner
            .send(Method::POST, EP_BULLET_PUBLIC, None, Some("{}".to_string()), false)
            .await
    }

    // -------------------------------------------------------------------------------------------
    // Private endpoints
    // -------------------------------------------------------------------------------------------

    /// Requests a private WebSocket bullet token (authenticated channels).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn request_bullet_private(&self) -> Result<KuCoinBulletToken, KuCoinHttpError> {
        self.inner
            .send(Method::POST, EP_BULLET_PRIVATE, None, Some("{}".to_string()), true)
            .await
    }

    /// Requests the current account state (spot `trade` balances).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn request_account_state(
        &self,
        account_id: AccountId,
    ) -> Result<AccountState, KuCoinHttpError> {
        let accounts: Vec<KuCoinAccount> = self
            .inner
            .send(Method::GET, EP_ACCOUNTS, Some("type=trade"), None, true)
            .await?;
        parse_account_state(&accounts, account_id, self.ts_init())
            .map_err(|e| KuCoinHttpError::ValidationError(e.to_string()))
    }

    /// Submits an order (raw JSON body per KuCoin's `POST /api/v1/orders`).
    ///
    /// Returns the assigned venue `orderId`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn submit_order(&self, body: String) -> Result<String, KuCoinHttpError> {
        let created: KuCoinOrderCreated = self
            .inner
            .send(Method::POST, EP_ORDERS, None, Some(body), true)
            .await?;
        Ok(created.order_id)
    }

    /// Cancels an order by its venue `orderId`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn cancel_order(&self, order_id: &str) -> Result<(), KuCoinHttpError> {
        let endpoint = format!("{EP_ORDERS}/{order_id}");
        let _: serde_json::Value = self
            .inner
            .send(Method::DELETE, &endpoint, None, None, true)
            .await?;
        Ok(())
    }

    /// Requests a single order's status report by venue `orderId`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn request_order_status(
        &self,
        order_id: &str,
        account_id: AccountId,
    ) -> Result<OrderStatusReport, KuCoinHttpError> {
        let endpoint = format!("{EP_ORDERS}/{order_id}");
        let order: KuCoinOrder = self
            .inner
            .send(Method::GET, &endpoint, None, None, true)
            .await?;
        let instrument_id = crate::common::parse::instrument_id_from_kucoin_symbol(&order.symbol);
        let (price_precision, size_precision) = self.precisions(&instrument_id);
        parse_order_status_report(
            &order,
            account_id,
            price_precision,
            size_precision,
            self.ts_init(),
        )
        .map_err(|e| KuCoinHttpError::ValidationError(e.to_string()))
    }

    /// Requests recent fills (executions) as [`FillReport`]s.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<FillReport>, KuCoinHttpError> {
        // The fills endpoint returns a paginated `{items:[...]}` wrapper.
        let page: super::models::KuCoinPaginated<KuCoinFill> = self
            .inner
            .send(Method::GET, EP_FILLS, None, None, true)
            .await?;
        let ts_init = self.ts_init();
        let mut reports = Vec::with_capacity(page.items.len());
        for fill in &page.items {
            let instrument_id =
                crate::common::parse::instrument_id_from_kucoin_symbol(&fill.symbol);
            let (price_precision, size_precision) = self.precisions(&instrument_id);
            match parse_fill_report(fill, account_id, price_precision, size_precision, ts_init) {
                Ok(r) => reports.push(r),
                Err(e) => log::warn!("Skipping fill: {e}"),
            }
        }
        Ok(reports)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn build_public_client() {
        let client = KuCoinHttpClient::new(None).unwrap();
        assert!(!client.has_credentials());
    }

    #[rstest]
    fn build_signed_client() {
        let client = KuCoinHttpClient::with_credentials(
            "k".to_string(),
            "s".to_string(),
            "p".to_string(),
            None,
        )
        .unwrap();
        assert!(client.has_credentials());
    }
}
