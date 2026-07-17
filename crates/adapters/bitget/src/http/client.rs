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

//! Ergonomic wrapper around the **Bitget v2 REST API** (<https://www.bitget.com/api-doc>).
//!
//! Handles request signing (HMAC-SHA256 base64), the `code == "00000"` success envelope, and
//! deserialization into nautilus domain types. Signing is ported first-hand from CCXT
//! `bitget.py::sign()`.

use std::{collections::HashMap, num::NonZeroU32};

use async_trait::async_trait;
use chrono::Utc;
use nautilus_common::providers::{InstrumentProvider, InstrumentStore};
use nautilus_core::{
    UnixNanos, consts::NAUTILUS_USER_AGENT, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    identifiers::{AccountId, InstrumentId},
    instruments::InstrumentAny,
};
use nautilus_network::{
    http::{HttpClient, Method, USER_AGENT},
    ratelimiter::quota::Quota,
};
use serde::de::DeserializeOwned;

use super::{
    error::BitgetHttpError,
    models::{
        BitgetCancelOrderRequest, BitgetContractSymbol, BitgetOrder, BitgetPlaceOrderRequest,
        BitgetPlaceOrderResponse, BitgetResponse, BitgetSpotAsset, BitgetSpotSymbol, BitgetTrade,
    },
    parse::{
        parse_order_status_report, parse_perpetual_instrument, parse_spot_account_state,
        parse_spot_instrument, parse_trade_tick,
    },
};
use crate::common::{
    consts::{
        BITGET_API_PREFIX, BITGET_BROKER_ID, BITGET_HTTP_URL, BITGET_REST_RATE_LIMIT_PER_SEC,
        BITGET_SUCCESS_CODE,
    },
    credential::Credential,
    enums::BitgetProductType,
};

const BITGET_RATE_KEY: &str = "bitget:global";

/// HTTP client for the Bitget v2 REST API.
pub struct BitgetHttpClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
    instruments: InstrumentStore,
    timeout_secs: u64,
}

impl std::fmt::Debug for BitgetHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BitgetHttpClient")
            .field("base_url", &self.base_url)
            .field("has_credential", &self.credential.is_some())
            .finish()
    }
}

impl BitgetHttpClient {
    /// Creates a new public (unauthenticated) [`BitgetHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn new(base_url: Option<String>, timeout_secs: Option<u64>) -> anyhow::Result<Self> {
        Self::build(base_url, timeout_secs, None)
    }

    /// Creates a new authenticated [`BitgetHttpClient`] from explicit credentials or environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn with_credentials(
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> anyhow::Result<Self> {
        let credential = Credential::resolve(api_key, api_secret, api_passphrase);
        Self::build(base_url, timeout_secs, credential)
    }

    fn build(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        credential: Option<Credential>,
    ) -> anyhow::Result<Self> {
        let timeout_secs = timeout_secs.unwrap_or(60);
        let quota = Quota::per_second(
            NonZeroU32::new(BITGET_REST_RATE_LIMIT_PER_SEC).expect("non-zero rate limit"),
        )
        .expect("valid quota");
        let headers =
            HashMap::from([(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())]);
        let client = HttpClient::new(
            headers,
            vec![],
            vec![(BITGET_RATE_KEY.to_string(), quota)],
            Some(quota),
            Some(timeout_secs),
            None,
        )
        .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?;

        Ok(Self {
            base_url: base_url.unwrap_or_else(|| BITGET_HTTP_URL.to_string()),
            client,
            credential,
            instruments: InstrumentStore::new(),
            timeout_secs,
        })
    }

    /// Returns `true` if the client has credentials configured.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.credential.is_some()
    }

    /// Builds the sorted raw query string (CCXT `keysort` + `rawencode`).
    ///
    /// Bitget signs the *sorted* query with raw (non-percent-encoded) values. All Bitget query
    /// params are ASCII, so the raw string is also URL-safe and is reused verbatim in the URL.
    fn build_query(mut params: Vec<(String, String)>) -> String {
        params.sort_by(|a, b| a.0.cmp(&b.0));
        params
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&")
    }

    /// Signs and sends a request, returning the deserialized `data` field.
    async fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: Vec<(String, String)>,
        body: Option<Vec<u8>>,
        authenticate: bool,
    ) -> Result<Option<T>, BitgetHttpError> {
        let query_string = Self::build_query(query);
        let request_path = if query_string.is_empty() {
            format!("{BITGET_API_PREFIX}{path}")
        } else {
            format!("{BITGET_API_PREFIX}{path}?{query_string}")
        };
        let url = format!("{}{request_path}", self.base_url);

        let mut headers = HashMap::new();
        if authenticate {
            let credential = self
                .credential
                .as_ref()
                .ok_or(BitgetHttpError::MissingCredentials)?;
            let timestamp = Utc::now().timestamp_millis().to_string();
            let signature =
                credential.sign_bytes(&timestamp, method.as_str(), &request_path, body.as_deref());
            headers.insert("ACCESS-KEY".to_string(), credential.api_key().to_string());
            headers.insert("ACCESS-SIGN".to_string(), signature);
            headers.insert("ACCESS-TIMESTAMP".to_string(), timestamp);
            headers.insert(
                "ACCESS-PASSPHRASE".to_string(),
                credential.api_passphrase().to_string(),
            );
            headers.insert("X-CHANNEL-API-CODE".to_string(), BITGET_BROKER_ID.to_string());
        }
        if body.is_some() {
            headers.insert("Content-Type".to_string(), "application/json".to_string());
        }

        let resp = self
            .client
            .request(
                method,
                url,
                None,
                Some(headers),
                body,
                Some(self.timeout_secs),
                Some(vec![BITGET_RATE_KEY.to_string()]),
            )
            .await?;

        if !resp.status.is_success() {
            let body = String::from_utf8_lossy(&resp.body).to_string();
            return Err(BitgetHttpError::UnexpectedStatus {
                status: resp.status.as_u16(),
                body,
            });
        }

        let parsed: BitgetResponse<T> = serde_json::from_slice(&resp.body)
            .map_err(|e| BitgetHttpError::JsonError(e.to_string()))?;
        if parsed.code != BITGET_SUCCESS_CODE {
            return Err(BitgetHttpError::BitgetError {
                code: parsed.code,
                message: parsed.msg,
            });
        }
        Ok(parsed.data)
    }

    fn ts_init() -> UnixNanos {
        get_atomic_clock_realtime().get_time_ns()
    }

    // -- Market data -----------------------------------------------------------------------------

    /// Fetches all spot symbols (`GET /api/v2/spot/public/symbols`).
    ///
    /// # Errors
    ///
    /// Returns an error if the request or deserialization fails.
    pub async fn get_spot_symbols(&self) -> Result<Vec<BitgetSpotSymbol>, BitgetHttpError> {
        let data = self
            .send::<Vec<BitgetSpotSymbol>>(Method::GET, "/v2/spot/public/symbols", vec![], None, false)
            .await?;
        Ok(data.unwrap_or_default())
    }

    /// Fetches all contracts for a futures product type (`GET /api/v2/mix/market/contracts`).
    ///
    /// # Errors
    ///
    /// Returns an error if the request or deserialization fails.
    pub async fn get_contract_symbols(
        &self,
        product: BitgetProductType,
    ) -> Result<Vec<BitgetContractSymbol>, BitgetHttpError> {
        let query = vec![("productType".to_string(), product.to_string())];
        let data = self
            .send::<Vec<BitgetContractSymbol>>(
                Method::GET,
                "/v2/mix/market/contracts",
                query,
                None,
                false,
            )
            .await?;
        Ok(data.unwrap_or_default())
    }

    /// Fetches recent public spot trades (`GET /api/v2/spot/market/fills`).
    ///
    /// # Errors
    ///
    /// Returns an error if the request or deserialization fails.
    pub async fn get_spot_trades(
        &self,
        instrument_id: InstrumentId,
        raw_symbol: &str,
        price_precision: u8,
        size_precision: u8,
        limit: Option<u32>,
    ) -> Result<Vec<nautilus_model::data::TradeTick>, BitgetHttpError> {
        let mut query = vec![("symbol".to_string(), raw_symbol.to_string())];
        if let Some(l) = limit {
            query.push(("limit".to_string(), l.to_string()));
        }
        let raw = self
            .send::<Vec<BitgetTrade>>(Method::GET, "/v2/spot/market/fills", query, None, false)
            .await?
            .unwrap_or_default();
        let ts_init = Self::ts_init();
        let mut ticks = Vec::with_capacity(raw.len());
        for t in &raw {
            match parse_trade_tick(t, instrument_id, price_precision, size_precision, ts_init) {
                Ok(tick) => ticks.push(tick),
                Err(e) => log::warn!("Failed to parse trade: {e}"),
            }
        }
        Ok(ticks)
    }

    // -- Account / trading -----------------------------------------------------------------------

    /// Fetches spot account balances (`GET /api/v2/spot/account/assets`).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn get_spot_account_state(
        &self,
        account_id: AccountId,
    ) -> Result<nautilus_model::events::AccountState, BitgetHttpError> {
        let assets = self
            .send::<Vec<BitgetSpotAsset>>(Method::GET, "/v2/spot/account/assets", vec![], None, true)
            .await?
            .unwrap_or_default();
        parse_spot_account_state(&assets, account_id, Self::ts_init())
            .map_err(|e| BitgetHttpError::JsonError(e.to_string()))
    }

    /// Fetches unfilled spot orders (`GET /api/v2/spot/trade/unfilled-orders`).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn get_spot_open_orders(
        &self,
        account_id: AccountId,
        price_precision: u8,
        size_precision: u8,
    ) -> Result<Vec<nautilus_model::reports::OrderStatusReport>, BitgetHttpError> {
        let orders = self
            .send::<Vec<BitgetOrder>>(
                Method::GET,
                "/v2/spot/trade/unfilled-orders",
                vec![],
                None,
                true,
            )
            .await?
            .unwrap_or_default();
        let ts_init = Self::ts_init();
        let mut reports = Vec::with_capacity(orders.len());
        for o in &orders {
            match parse_order_status_report(
                o,
                account_id,
                BitgetProductType::Spot,
                price_precision,
                size_precision,
                ts_init,
            ) {
                Ok(r) => reports.push(r),
                Err(e) => log::warn!("Failed to parse order: {e}"),
            }
        }
        Ok(reports)
    }

    /// Places a spot order (`POST /api/v2/spot/trade/place-order`).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn place_spot_order(
        &self,
        request: &BitgetPlaceOrderRequest,
    ) -> Result<BitgetPlaceOrderResponse, BitgetHttpError> {
        let body = serde_json::to_vec(request)?;
        let data = self
            .send::<BitgetPlaceOrderResponse>(
                Method::POST,
                "/v2/spot/trade/place-order",
                vec![],
                Some(body),
                true,
            )
            .await?;
        data.ok_or_else(|| BitgetHttpError::JsonError("empty place-order response".to_string()))
    }

    /// Cancels a spot order (`POST /api/v2/spot/trade/cancel-order`).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn cancel_spot_order(
        &self,
        request: &BitgetCancelOrderRequest,
    ) -> Result<(), BitgetHttpError> {
        let body = serde_json::to_vec(request)?;
        let _ = self
            .send::<serde_json::Value>(
                Method::POST,
                "/v2/spot/trade/cancel-order",
                vec![],
                Some(body),
                true,
            )
            .await?;
        Ok(())
    }
}

#[async_trait(?Send)]
impl InstrumentProvider for BitgetHttpClient {
    fn store(&self) -> &InstrumentStore {
        &self.instruments
    }

    fn store_mut(&mut self) -> &mut InstrumentStore {
        &mut self.instruments
    }

    async fn load_all(
        &mut self,
        _filters: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let ts_init = Self::ts_init();
        let mut loaded: Vec<InstrumentAny> = Vec::new();

        // Spot markets.
        for def in self.get_spot_symbols().await? {
            if def.status != "online" {
                continue;
            }
            match parse_spot_instrument(&def, ts_init) {
                Ok(inst) => loaded.push(inst),
                Err(e) => log::warn!("Skipping spot {}: {e}", def.symbol),
            }
        }

        // USDT-margined perpetual markets.
        for def in self
            .get_contract_symbols(BitgetProductType::UsdtFutures)
            .await?
        {
            if def.symbol_type != "perpetual" || def.symbol_status != "normal" {
                continue;
            }
            match parse_perpetual_instrument(&def, BitgetProductType::UsdtFutures, ts_init) {
                Ok(inst) => loaded.push(inst),
                Err(e) => log::warn!("Skipping contract {}: {e}", def.symbol),
            }
        }

        for inst in loaded {
            self.instruments.add(inst);
        }
        self.instruments.set_initialized();
        Ok(())
    }

    async fn load(
        &mut self,
        instrument_id: &InstrumentId,
        filters: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        if self.instruments.is_empty() {
            self.load_all(filters).await?;
        }
        if self.instruments.find(instrument_id).is_none() {
            anyhow::bail!("Instrument {instrument_id} not found on Bitget");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_build_query_sorts_keys() {
        let q = BitgetHttpClient::build_query(vec![
            ("symbol".to_string(), "BTCUSDT".to_string()),
            ("granularity".to_string(), "1min".to_string()),
            ("limit".to_string(), "100".to_string()),
        ]);
        assert_eq!(q, "granularity=1min&limit=100&symbol=BTCUSDT");
    }

    #[rstest]
    fn test_public_client_builds() {
        let client = BitgetHttpClient::new(None, None).unwrap();
        assert!(!client.has_credentials());
        assert_eq!(client.base_url, BITGET_HTTP_URL);
    }
}
