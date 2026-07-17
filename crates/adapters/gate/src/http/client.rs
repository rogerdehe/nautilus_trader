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

//! HTTP client for the Gate (gate.io) APIv4 REST interface.
//!
//! Signing follows CCXT `gate.py::sign` exactly (see [`crate::common::credential`]):
//! HMAC-SHA512 over `METHOD\n/api/v4/<type>/<path>\n<raw_query>\nSHA512(body)\n<timestamp_secs>`.

use std::{collections::HashMap, num::NonZeroU32, sync::LazyLock};

use nautilus_core::{UnixNanos, consts::NAUTILUS_USER_AGENT, time::get_atomic_clock_realtime};
use nautilus_model::instruments::InstrumentAny;
use nautilus_network::{
    http::{HttpClient, Method, USER_AGENT},
    ratelimiter::quota::Quota,
};
use serde::de::DeserializeOwned;

use crate::{
    common::{
        consts::{
            EP_SPOT_ACCOUNTS, EP_SPOT_CURRENCY_PAIRS, EP_SPOT_ORDER_BOOK, EP_SPOT_ORDERS,
            EP_SPOT_TRADES, GATE_HTTP_BASE_URL, GATE_SIGN_PREFIX, GATE_TYPE_SPOT,
        },
        credential::Credential,
    },
    http::{
        error::{Error, Result},
        models::{
            SpotAccount, SpotCurrencyPair, SpotOrder, SpotOrderBook, SpotOrderRequest, SpotTrade,
        },
        parse::{parse_order_book_snapshot, parse_spot_instrument, parse_trade_tick},
    },
};

/// Default Gate REST rate limit (~20 req/s, from CCXT `rateLimit = 50` ms/token).
pub static GATE_REST_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(20).expect("non-zero")).expect("valid quota")
});

/// HTTP client for Gate APIv4.
#[derive(Debug, Clone)]
pub struct GateHttpClient {
    client: HttpClient,
    credential: Option<Credential>,
    base_url: String,
}

impl GateHttpClient {
    /// Creates a new public (unauthenticated) client against the default host.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be constructed.
    pub fn new(timeout_secs: Option<u64>, proxy_url: Option<String>) -> anyhow::Result<Self> {
        Self::with_optional_credentials(None, timeout_secs, proxy_url)
    }

    /// Creates a client, attaching credentials for signed endpoints when provided.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be constructed.
    pub fn with_optional_credentials(
        credential: Option<Credential>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> anyhow::Result<Self> {
        let client = HttpClient::new(
            Self::default_headers(),
            vec![],
            vec![],
            Some(*GATE_REST_QUOTA),
            timeout_secs,
            proxy_url,
        )?;
        Ok(Self {
            client,
            credential,
            base_url: GATE_HTTP_BASE_URL.to_string(),
        })
    }

    /// Overrides the base host (used by tests against a mock server).
    pub fn set_base_url(&mut self, url: String) {
        self.base_url = url;
    }

    /// Returns `true` when the client carries credentials for signed requests.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.credential.is_some()
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([
            (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ])
    }

    /// `entirePath` per CCXT = `/<type>/<path>` (e.g. `/spot/currency_pairs`).
    fn entire_path(product_type: &str, path: &str) -> String {
        if path.is_empty() {
            format!("/{product_type}")
        } else {
            format!("/{product_type}/{path}")
        }
    }

    fn full_url(&self, entire_path: &str, query: &str) -> String {
        if query.is_empty() {
            format!("{}{GATE_SIGN_PREFIX}{entire_path}", self.base_url)
        } else {
            format!("{}{GATE_SIGN_PREFIX}{entire_path}?{query}", self.base_url)
        }
    }

    /// Sends an unauthenticated GET and deserializes the JSON body.
    async fn get_public<T: DeserializeOwned>(
        &self,
        product_type: &str,
        path: &str,
        query: &str,
    ) -> Result<T> {
        let entire_path = Self::entire_path(product_type, path);
        let url = self.full_url(&entire_path, query);
        let resp = self
            .client
            .request(Method::GET, url, None, None, None, None, None)
            .await
            .map_err(Error::from_http_client)?;
        if !resp.status.is_success() {
            return Err(Error::from_http_status(resp.status.as_u16(), &resp.body));
        }
        serde_json::from_slice(&resp.body).map_err(Error::Serde)
    }

    /// Sends a signed request (private endpoints) and deserializes the JSON body.
    ///
    /// `query` is the RAW (non-url-encoded) query string used for BOTH the URL and the signature
    /// payload; keep values free of reserved characters (Gate ids never contain them).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] when no credentials are configured, or a transport/status error.
    pub async fn request_signed<T: DeserializeOwned>(
        &self,
        method: Method,
        product_type: &str,
        path: &str,
        query: &str,
        body: Option<String>,
    ) -> Result<T> {
        let credential = self
            .credential
            .as_ref()
            .ok_or_else(|| Error::auth("no credentials configured for signed request"))?;

        let entire_path = Self::entire_path(product_type, path);
        let signature_path = format!("{GATE_SIGN_PREFIX}{entire_path}");
        let body_str = body.clone().unwrap_or_default();
        let timestamp = (get_atomic_clock_realtime().get_time_ns().as_u64() / 1_000_000_000)
            .to_string();
        let method_str = method.as_str();

        let signature =
            credential.sign_request(method_str, &signature_path, query, &body_str, &timestamp);

        let headers = HashMap::from([
            ("KEY".to_string(), credential.api_key().to_string()),
            ("Timestamp".to_string(), timestamp),
            ("SIGN".to_string(), signature),
            ("Content-Type".to_string(), "application/json".to_string()),
        ]);

        let url = self.full_url(&entire_path, query);
        let body_bytes = body.map(String::into_bytes);
        let resp = self
            .client
            .request(method, url, None, Some(headers), body_bytes, None, None)
            .await
            .map_err(Error::from_http_client)?;
        if !resp.status.is_success() {
            return Err(Error::from_http_status(resp.status.as_u16(), &resp.body));
        }
        serde_json::from_slice(&resp.body).map_err(Error::Serde)
    }

    // -- Domain methods ---------------------------------------------------------------------------

    /// Fetches all Gate spot markets and converts them to Nautilus instruments.
    ///
    /// # Errors
    ///
    /// Returns an error on transport/status failure or a malformed response.
    pub async fn request_instruments(&self) -> Result<Vec<InstrumentAny>> {
        let markets: Vec<SpotCurrencyPair> = self
            .get_public(GATE_TYPE_SPOT, EP_SPOT_CURRENCY_PAIRS, "")
            .await?;
        let ts_init = self.clock_ns();
        let mut out = Vec::with_capacity(markets.len());
        for market in &markets {
            // Skip non-tradable/malformed rows rather than aborting the whole load.
            if market.precision == 0 && market.amount_precision == 0 {
                continue;
            }
            if let Ok(inst) = parse_spot_instrument(market, ts_init) {
                out.push(inst);
            }
        }
        Ok(out)
    }

    /// Fetches recent spot trades for `currency_pair` (e.g. `BTC_USDT`).
    ///
    /// # Errors
    ///
    /// Returns an error on transport/status failure or a malformed response.
    pub async fn request_trades(
        &self,
        currency_pair: &str,
        price_precision: u8,
        size_precision: u8,
    ) -> Result<Vec<nautilus_model::data::TradeTick>> {
        let query = format!("currency_pair={currency_pair}");
        let trades: Vec<SpotTrade> = self
            .get_public(GATE_TYPE_SPOT, EP_SPOT_TRADES, &query)
            .await?;
        let instrument_id = crate::common::parse::instrument_id_from_spot_symbol(currency_pair);
        let ts_init = self.clock_ns();
        let mut out = Vec::with_capacity(trades.len());
        for t in &trades {
            if let Ok(tick) =
                parse_trade_tick(t, instrument_id, price_precision, size_precision, ts_init)
            {
                out.push(tick);
            }
        }
        Ok(out)
    }

    /// Fetches a spot order book snapshot for `currency_pair`.
    ///
    /// # Errors
    ///
    /// Returns an error on transport/status failure or a malformed response.
    pub async fn request_order_book_snapshot(
        &self,
        currency_pair: &str,
        price_precision: u8,
        size_precision: u8,
        limit: u32,
    ) -> Result<nautilus_model::data::OrderBookDeltas> {
        let query = format!("currency_pair={currency_pair}&limit={limit}");
        let book: SpotOrderBook = self
            .get_public(GATE_TYPE_SPOT, EP_SPOT_ORDER_BOOK, &query)
            .await?;
        let instrument_id = crate::common::parse::instrument_id_from_spot_symbol(currency_pair);
        let ts_init = self.clock_ns();
        let ts_event = book
            .current
            .map(|ms| UnixNanos::from(ms as u64 * 1_000_000))
            .unwrap_or(ts_init);
        parse_order_book_snapshot(
            &book,
            instrument_id,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        )
        .map_err(|e| Error::transport(e.to_string()))
    }

    /// Places a spot order (`POST /spot/orders`, signed).
    ///
    /// # Errors
    ///
    /// Returns an error when unauthenticated or on a transport/status/parse failure.
    pub async fn place_spot_order(&self, request: &SpotOrderRequest) -> Result<SpotOrder> {
        let body = serde_json::to_string(request)?;
        self.request_signed(Method::POST, GATE_TYPE_SPOT, EP_SPOT_ORDERS, "", Some(body))
            .await
    }

    /// Cancels a spot order by venue id (`DELETE /spot/orders/{id}`, signed).
    ///
    /// # Errors
    ///
    /// Returns an error when unauthenticated or on a transport/status/parse failure.
    pub async fn cancel_spot_order(
        &self,
        order_id: &str,
        currency_pair: &str,
    ) -> Result<SpotOrder> {
        let path = format!("{EP_SPOT_ORDERS}/{order_id}");
        let query = format!("currency_pair={currency_pair}");
        self.request_signed(Method::DELETE, GATE_TYPE_SPOT, &path, &query, None)
            .await
    }

    /// Fetches spot account balances (`GET /spot/accounts`, signed).
    ///
    /// # Errors
    ///
    /// Returns an error when unauthenticated or on a transport/status/parse failure.
    pub async fn request_spot_accounts(&self) -> Result<Vec<SpotAccount>> {
        self.request_signed(Method::GET, GATE_TYPE_SPOT, EP_SPOT_ACCOUNTS, "", None)
            .await
    }

    fn clock_ns(&self) -> UnixNanos {
        get_atomic_clock_realtime().get_time_ns()
    }
}
