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

//! BingX HTTP client: signed request builder + REST endpoints + instrument provider.
//!
//! Signing mirrors CCXT `bingx.sign` (see [`crate::common::credential`]): all business params plus a
//! `timestamp` are sorted, `HMAC_SHA256`'d, and the `signature` is appended to the query string with
//! auth carried by the `X-BX-APIKEY` header. BingX places request params in the query string for
//! both GET and POST, so no request body is used.

use std::{collections::BTreeMap, collections::HashMap, num::NonZeroU32, sync::Arc};

use async_trait::async_trait;
use nautilus_common::providers::{InstrumentProvider, InstrumentStore};
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{identifiers::InstrumentId, instruments::InstrumentAny};
use nautilus_network::{
    http::{HttpClient, Method},
    ratelimiter::quota::Quota,
};
use serde::de::DeserializeOwned;

use super::{
    error::BingXHttpError,
    models::{BingXResponse, BingXSpotSymbols, BingXSpotTrade},
    parse::{parse_spot_instrument, parse_spot_trade_tick},
};
use crate::common::{
    consts::{
        BINGX_HTTP_BASE_URL, BINGX_HTTP_TIMEOUT_SECS, BINGX_REST_RATE_LIMIT_PER_SEC,
        EP_SPOT_SYMBOLS, EP_SPOT_TRADES, HEADER_API_KEY, HEADER_SOURCE_KEY, SOURCE_KEY_VALUE,
    },
    credential::Credential,
};

/// A thin, first-hand BingX REST client built on the shared Nautilus HTTP transport.
#[derive(Debug, Clone)]
pub struct BingXHttpClient {
    inner: Arc<HttpClient>,
    base_url: String,
    credential: Option<Credential>,
    timeout_secs: u64,
}

impl BingXHttpClient {
    /// Creates a new public (unauthenticated) [`BingXHttpClient`].
    #[must_use]
    pub fn new(base_url: Option<String>, timeout_secs: Option<u64>) -> Self {
        let quota = Quota::per_second(
            NonZeroU32::new(BINGX_REST_RATE_LIMIT_PER_SEC).expect("non-zero rate limit"),
        );
        let inner = HttpClient::new(HashMap::new(), Vec::new(), Vec::new(), quota, None, None)
            .expect("failed to build BingX HTTP client");
        Self {
            inner: Arc::new(inner),
            base_url: base_url.unwrap_or_else(|| BINGX_HTTP_BASE_URL.to_string()),
            credential: None,
            timeout_secs: timeout_secs.unwrap_or(BINGX_HTTP_TIMEOUT_SECS),
        }
    }

    /// Creates a new signed [`BingXHttpClient`] from raw credentials.
    #[must_use]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> Self {
        let mut client = Self::new(base_url, timeout_secs);
        client.credential = Some(Credential::new(api_key, api_secret));
        client
    }

    /// Returns `true` if the client can sign private requests.
    #[must_use]
    pub const fn has_credentials(&self) -> bool {
        self.credential.is_some()
    }

    fn full_url(&self, endpoint: &str) -> String {
        format!("{}/{}", self.base_url, endpoint)
    }

    /// Sends an unsigned public request and deserializes the `{code,msg,data}` envelope.
    async fn get_public<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        params: &[(&str, String)],
    ) -> Result<T, BingXHttpError> {
        let query = encode_query(params);
        let url = if query.is_empty() {
            self.full_url(endpoint)
        } else {
            format!("{}?{}", self.full_url(endpoint), query)
        };
        self.execute(Method::GET, url, None).await
    }

    /// Sends a signed private request. `params` are the business params; `timestamp` + `signature`
    /// are added here. Auth is carried by the `X-BX-APIKEY` header.
    #[allow(dead_code)]
    async fn send_signed<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        params: &[(&str, String)],
    ) -> Result<T, BingXHttpError> {
        let credential = self
            .credential
            .as_ref()
            .ok_or(BingXHttpError::MissingCredentials)?;

        let mut sorted: BTreeMap<String, String> = BTreeMap::new();
        for (k, v) in params {
            sorted.insert((*k).to_string(), v.clone());
        }
        let ts = get_atomic_clock_realtime().get_time_ms();
        sorted.insert("timestamp".to_string(), ts.to_string());

        let to_sign = Credential::string_to_sign(&sorted);
        let signature = credential.sign(&to_sign);

        let query = encode_query_map(&sorted);
        let url = format!("{}?{}&signature={}", self.full_url(endpoint), query, signature);

        let mut headers = HashMap::new();
        headers.insert(HEADER_API_KEY.to_string(), credential.api_key().to_string());
        headers.insert(HEADER_SOURCE_KEY.to_string(), SOURCE_KEY_VALUE.to_string());

        self.execute(method, url, Some(headers)).await
    }

    async fn execute<T: DeserializeOwned>(
        &self,
        method: Method,
        url: String,
        headers: Option<HashMap<String, String>>,
    ) -> Result<T, BingXHttpError> {
        let resp = self
            .inner
            .request(method, url, None, headers, None, Some(self.timeout_secs), None)
            .await
            .map_err(|e| BingXHttpError::Network(e.to_string()))?;

        if !resp.status.is_success() {
            return Err(BingXHttpError::Status {
                status: resp.status.as_u16(),
                body: String::from_utf8_lossy(&resp.body).chars().take(512).collect(),
            });
        }

        let envelope: BingXResponse<T> = serde_json::from_slice(&resp.body)?;
        if envelope.code != 0 {
            return Err(BingXHttpError::Api {
                code: envelope.code,
                msg: envelope.msg,
            });
        }
        envelope
            .data
            .ok_or_else(|| BingXHttpError::Json("missing `data` in response".to_string()))
    }

    /// Fetches all spot markets and builds Nautilus instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or a market cannot be parsed.
    pub async fn request_spot_instruments(&self) -> Result<Vec<InstrumentAny>, BingXHttpError> {
        let symbols: BingXSpotSymbols = self.get_public(EP_SPOT_SYMBOLS, &[]).await?;
        let ts_init = UnixNanos::from(get_atomic_clock_realtime().get_time_ns());
        let mut out = Vec::with_capacity(symbols.symbols.len());
        for s in &symbols.symbols {
            // Skip markets that are offline or not API-tradable.
            if s.status != 1 || !(s.api_state_buy && s.api_state_sell) {
                continue;
            }
            match parse_spot_instrument(
                &s.symbol,
                s.tick_size,
                s.step_size,
                s.min_notional,
                s.max_notional,
                ts_init,
            ) {
                Ok(inst) => out.push(inst),
                Err(e) => log::warn!("Skipping BingX spot market {}: {e}", s.symbol),
            }
        }
        Ok(out)
    }

    /// Fetches recent public trades for a spot symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn request_spot_trades(
        &self,
        symbol: &str,
        instrument_id: InstrumentId,
        price_precision: u8,
        size_precision: u8,
        limit: Option<u32>,
    ) -> Result<Vec<nautilus_model::data::TradeTick>, BingXHttpError> {
        let mut params: Vec<(&str, String)> = vec![("symbol", symbol.to_string())];
        if let Some(limit) = limit {
            params.push(("limit", limit.to_string()));
        }
        let trades: Vec<BingXSpotTrade> = self.get_public(EP_SPOT_TRADES, &params).await?;
        let ts_init = UnixNanos::from(get_atomic_clock_realtime().get_time_ns());
        let mut out = Vec::with_capacity(trades.len());
        for t in &trades {
            match parse_spot_trade_tick(
                instrument_id,
                t.price,
                t.qty,
                t.id,
                t.time,
                t.buyer_maker,
                price_precision,
                size_precision,
                ts_init,
            ) {
                Ok(tick) => out.push(tick),
                Err(e) => log::warn!("Skipping BingX trade for {symbol}: {e}"),
            }
        }
        Ok(out)
    }
}

/// Percent-encodes an ordered list of `(key, value)` pairs into a query string.
fn encode_query(params: &[(&str, String)]) -> String {
    serde_urlencoded::to_string(params).unwrap_or_default()
}

/// Percent-encodes a sorted map into a query string, preserving the (already sorted) key order.
fn encode_query_map(params: &BTreeMap<String, String>) -> String {
    let pairs: Vec<(&str, &str)> = params.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    serde_urlencoded::to_string(pairs).unwrap_or_default()
}

/// Loads BingX instruments into an [`InstrumentStore`] for the data/execution clients.
#[derive(Debug)]
pub struct BingXInstrumentProvider {
    client: BingXHttpClient,
    store: InstrumentStore,
}

impl BingXInstrumentProvider {
    /// Creates a new [`BingXInstrumentProvider`] wrapping the given HTTP client.
    #[must_use]
    pub fn new(client: BingXHttpClient) -> Self {
        Self {
            client,
            store: InstrumentStore::new(),
        }
    }
}

#[async_trait(?Send)]
impl InstrumentProvider for BingXInstrumentProvider {
    fn store(&self) -> &InstrumentStore {
        &self.store
    }

    fn store_mut(&mut self) -> &mut InstrumentStore {
        &mut self.store
    }

    async fn load_all(
        &mut self,
        _filters: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let instruments = self
            .client
            .request_spot_instruments()
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        self.store.add_bulk(instruments);
        Ok(())
    }

    async fn load(
        &mut self,
        instrument_id: &InstrumentId,
        filters: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        // BingX has no single-symbol markets endpoint; load all then keep the requested id.
        if self.store.find(instrument_id).is_none() {
            self.load_all(filters).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_client_has_no_credentials() {
        let client = BingXHttpClient::new(None, None);
        assert!(!client.has_credentials());
        assert_eq!(client.base_url, BINGX_HTTP_BASE_URL);
    }

    #[test]
    fn signed_client_has_credentials() {
        let client =
            BingXHttpClient::with_credentials("k".to_string(), "s".to_string(), None, None);
        assert!(client.has_credentials());
    }

    #[test]
    fn query_encoding_sorted() {
        let mut m: BTreeMap<String, String> = BTreeMap::new();
        m.insert("symbol".to_string(), "BTC-USDT".to_string());
        m.insert("side".to_string(), "BUY".to_string());
        // sorted: side, symbol
        assert_eq!(encode_query_map(&m), "side=BUY&symbol=BTC-USDT");
    }
}
