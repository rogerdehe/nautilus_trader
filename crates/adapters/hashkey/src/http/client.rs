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

//! HTTP/REST client for the HashKey Global v1 API.

use std::{
    collections::HashMap,
    fmt::Debug,
    num::NonZeroU32,
    sync::LazyLock,
};

use nautilus_common::providers::{InstrumentProvider, InstrumentStore};
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    http::{HttpClient, Method},
    ratelimiter::quota::Quota,
};
use serde::de::DeserializeOwned;

use crate::{
    common::{
        consts::{
            EP_ACCOUNT, EP_DEPTH, EP_EXCHANGE_INFO, EP_SPOT_ORDER, EP_TIME, EP_TRADES,
            EP_USER_DATA_STREAM, HASHKEY_BROKER_ID, HASHKEY_HTTP_URL, HASHKEY_RATE_LIMIT_MS,
        },
        credential::{Credential, custom_urlencode},
    },
    http::{
        error::{HashKeyErrorResponse, HashKeyHttpError},
        models::{
            HashKeyAccount, HashKeyDepth, HashKeyExchangeInfo, HashKeyListenKey, HashKeyOrder,
            HashKeyServerTime, HashKeyTrade,
        },
        parse::parse_spot_instrument,
    },
};

const REDACTED: &str = "<redacted>";

/// HashKey REST rate limit — CCXT `describe().rateLimit` = 100ms (=> ~10 req/s global bucket).
static HASHKEY_REST_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    let per_sec = (1_000 / HASHKEY_RATE_LIMIT_MS.max(1)) as u32;
    Quota::per_second(NonZeroU32::new(per_sec.max(1)).expect("non-zero")).expect("valid quota")
});

/// A REST client for the HashKey Global exchange.
#[derive(Clone)]
pub struct HashKeyHttpClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
    broker_id: String,
}

impl Debug for HashKeyHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(HashKeyHttpClient))
            .field("base_url", &self.base_url)
            .field("credential", &self.credential.as_ref().map(|_| REDACTED))
            .finish_non_exhaustive()
    }
}

impl HashKeyHttpClient {
    /// Creates a new public (unauthenticated) [`HashKeyHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be constructed.
    pub fn new(base_url: Option<String>, timeout_secs: Option<u64>) -> anyhow::Result<Self> {
        Self::build(base_url, timeout_secs, None)
    }

    /// Creates a new authenticated [`HashKeyHttpClient`] from the given credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be constructed.
    pub fn with_credentials(
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> anyhow::Result<Self> {
        let credential = Credential::resolve(api_key, api_secret);
        Self::build(base_url, timeout_secs, credential)
    }

    fn build(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        credential: Option<Credential>,
    ) -> anyhow::Result<Self> {
        let client = HttpClient::new(
            HashMap::new(),
            vec![],
            vec![("hashkey:global".to_string(), *HASHKEY_REST_QUOTA)],
            Some(*HASHKEY_REST_QUOTA),
            timeout_secs,
            None,
        )
        .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?;

        Ok(Self {
            base_url: base_url.unwrap_or_else(|| HASHKEY_HTTP_URL.to_string()),
            client,
            credential,
            broker_id: HASHKEY_BROKER_ID.to_string(),
        })
    }

    /// Returns the configured REST base URL.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Returns `true` when credentials are configured.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.credential.is_some()
    }

    fn rate_keys() -> Vec<String> {
        vec!["hashkey:global".to_string()]
    }

    /// Sends an unauthenticated GET request and deserializes the JSON body into `T`.
    async fn get_public<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<T, HashKeyHttpError> {
        let mut url = format!("{}/{path}", self.base_url);
        if !query.is_empty() {
            url.push('?');
            url.push_str(&custom_urlencode(query));
        }
        let resp = self
            .client
            .request(Method::GET, url, None, None, None, None, Some(Self::rate_keys()))
            .await
            .map_err(|e| HashKeyHttpError::NetworkError(e.to_string()))?;
        Self::deserialize(&resp.status.is_success(), &resp.body)
    }

    /// Signs and sends an authenticated request. `business` params are appended after `timestamp`
    /// (insertion order preserved for signing). GET puts the signed query on the URL; other methods
    /// send it as the form-urlencoded body.
    async fn request_signed<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        business: Vec<(String, String)>,
    ) -> Result<T, HashKeyHttpError> {
        let credential = self
            .credential
            .as_ref()
            .ok_or(HashKeyHttpError::MissingCredentials)?;

        let timestamp = chrono::Utc::now().timestamp_millis().to_string();
        let mut params: Vec<(String, String)> = Vec::with_capacity(business.len() + 1);
        params.push(("timestamp".to_string(), timestamp));
        params.extend(business);

        let string_to_sign = custom_urlencode(&params);
        let signature = credential.sign(&string_to_sign);
        let signed_query = format!("{string_to_sign}&signature={signature}");

        let mut headers = HashMap::new();
        headers.insert("X-HK-APIKEY".to_string(), credential.api_key().to_string());
        headers.insert("INPUT-SOURCE".to_string(), self.broker_id.clone());
        headers.insert("broker_sign".to_string(), signature);

        let (url, body) = if method == Method::GET || method == Method::DELETE {
            (format!("{}/{path}?{signed_query}", self.base_url), None)
        } else {
            headers.insert(
                "Content-Type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            );
            (
                format!("{}/{path}", self.base_url),
                Some(signed_query.into_bytes()),
            )
        };

        let resp = self
            .client
            .request(method, url, None, Some(headers), body, None, Some(Self::rate_keys()))
            .await
            .map_err(|e| HashKeyHttpError::NetworkError(e.to_string()))?;
        Self::deserialize(&resp.status.is_success(), &resp.body)
    }

    fn deserialize<T: DeserializeOwned>(
        success: &bool,
        body: &[u8],
    ) -> Result<T, HashKeyHttpError> {
        // HashKey surfaces business errors as `{"code": ..., "msg": ...}` even on HTTP 200.
        if let Ok(err) = serde_json::from_slice::<HashKeyErrorResponse>(body) {
            let code = err.code.to_string();
            // A bare `{}` (ping) deserializes to neither error nor data cleanly; treat empty as ok.
            if code != "null" && !code.is_empty() {
                return Err(HashKeyHttpError::HashKeyError {
                    code,
                    message: err.msg,
                });
            }
        }
        if !success {
            return Err(HashKeyHttpError::UnexpectedStatus {
                status: 0,
                body: String::from_utf8_lossy(body).to_string(),
            });
        }
        serde_json::from_slice::<T>(body).map_err(|e| HashKeyHttpError::JsonError(e.to_string()))
    }

    // -- Public endpoints ------------------------------------------------------------------------

    /// Fetches the exchange server time (ms).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn get_server_time(&self) -> Result<i64, HashKeyHttpError> {
        let resp: HashKeyServerTime = self.get_public(EP_TIME, &[]).await?;
        Ok(resp.server_time)
    }

    /// Fetches the full exchange info (spot symbols + coins).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn get_exchange_info(&self) -> Result<HashKeyExchangeInfo, HashKeyHttpError> {
        self.get_public(EP_EXCHANGE_INFO, &[]).await
    }

    /// Fetches the order book snapshot for `symbol` (raw HashKey id, e.g. `BTCUSDT`).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn get_depth(
        &self,
        symbol: &str,
        limit: Option<u32>,
    ) -> Result<HashKeyDepth, HashKeyHttpError> {
        let mut query = vec![("symbol".to_string(), symbol.to_string())];
        if let Some(limit) = limit {
            query.push(("limit".to_string(), limit.to_string()));
        }
        self.get_public(EP_DEPTH, &query).await
    }

    /// Fetches recent public trades for `symbol`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn get_trades(
        &self,
        symbol: &str,
        limit: Option<u32>,
    ) -> Result<Vec<HashKeyTrade>, HashKeyHttpError> {
        let mut query = vec![("symbol".to_string(), symbol.to_string())];
        if let Some(limit) = limit {
            query.push(("limit".to_string(), limit.to_string()));
        }
        self.get_public(EP_TRADES, &query).await
    }

    /// Requests and parses all TRADING spot instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn request_instruments(&self) -> Result<Vec<InstrumentAny>, HashKeyHttpError> {
        let info = self.get_exchange_info().await?;
        let ts_init = UnixNanos::default();
        let mut out = Vec::with_capacity(info.symbols.len());
        for symbol in &info.symbols {
            if symbol.status != "TRADING" {
                continue;
            }
            match parse_spot_instrument(symbol, ts_init) {
                Ok(inst) => out.push(inst),
                Err(e) => log::warn!("Skipping HashKey instrument {}: {e}", symbol.symbol),
            }
        }
        Ok(out)
    }

    // -- Private endpoints -----------------------------------------------------------------------

    /// Fetches the spot account balances.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn get_account(&self) -> Result<HashKeyAccount, HashKeyHttpError> {
        self.request_signed(Method::GET, EP_ACCOUNT, vec![]).await
    }

    /// Submits a spot order. `business` carries the order parameters (symbol, side, type, quantity,
    /// price, newClientOrderId, timeInForce, …) in the order they should be signed.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn submit_order(
        &self,
        business: Vec<(String, String)>,
    ) -> Result<HashKeyOrder, HashKeyHttpError> {
        self.request_signed(Method::POST, EP_SPOT_ORDER, business)
            .await
    }

    /// Cancels a spot order by exchange `order_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn cancel_order(&self, order_id: &str) -> Result<HashKeyOrder, HashKeyHttpError> {
        self.request_signed(
            Method::DELETE,
            EP_SPOT_ORDER,
            vec![("orderId".to_string(), order_id.to_string())],
        )
        .await
    }

    /// Creates a user-data-stream listen key (private WebSocket auth).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn create_listen_key(&self) -> Result<String, HashKeyHttpError> {
        let resp: HashKeyListenKey = self
            .request_signed(Method::POST, EP_USER_DATA_STREAM, vec![])
            .await?;
        Ok(resp.listen_key)
    }

    /// Keeps a listen key alive (PUT `userDataStream`).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn keep_alive_listen_key(&self, listen_key: &str) -> Result<(), HashKeyHttpError> {
        let _: serde_json::Value = self
            .request_signed(
                Method::PUT,
                EP_USER_DATA_STREAM,
                vec![("listenKey".to_string(), listen_key.to_string())],
            )
            .await?;
        Ok(())
    }
}

/// Instrument provider backed by the HashKey `exchangeInfo` endpoint.
#[derive(Debug)]
pub struct HashKeyInstrumentProvider {
    client: HashKeyHttpClient,
    store: InstrumentStore,
}

impl HashKeyInstrumentProvider {
    /// Creates a new [`HashKeyInstrumentProvider`] wrapping `client`.
    #[must_use]
    pub fn new(client: HashKeyHttpClient) -> Self {
        Self {
            client,
            store: InstrumentStore::new(),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl InstrumentProvider for HashKeyInstrumentProvider {
    fn store(&self) -> &InstrumentStore {
        &self.store
    }

    fn store_mut(&mut self) -> &mut InstrumentStore {
        &mut self.store
    }

    async fn load_all(
        &mut self,
        _filters: Option<&std::collections::HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let instruments = self.client.request_instruments().await?;
        self.store.add_bulk(instruments);
        self.store.set_initialized();
        Ok(())
    }

    async fn load(
        &mut self,
        instrument_id: &InstrumentId,
        _filters: Option<&std::collections::HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let instruments = self.client.request_instruments().await?;
        if let Some(found) = instruments
            .into_iter()
            .find(|i| &i.id() == instrument_id)
        {
            self.store.add(found);
        }
        Ok(())
    }
}
