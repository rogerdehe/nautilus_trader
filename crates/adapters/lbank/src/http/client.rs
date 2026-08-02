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

//! HTTP client for the LBank v2 spot REST interface.
//!
//! Public GETs and signed POSTs both return the `{result,data,error_code,msg}` envelope; the client
//! checks `error_code` even on HTTP 200 (LBank reports logical failures that way). Signing follows
//! CCXT `lbank.py::sign` exactly (see [`crate::common::credential`]): the signed system params
//! (`api_key`/`echostr`/`signature_method`/`timestamp`) are generated ONCE and reused across the
//! signed string, the urlencoded body, and the auth headers.

use std::{collections::BTreeMap, num::NonZeroU32, sync::LazyLock};

use nautilus_core::{UnixNanos, consts::NAUTILUS_USER_AGENT, time::get_atomic_clock_realtime};
use nautilus_model::{data::TradeTick, instruments::InstrumentAny};
use nautilus_network::{
    http::{HttpClient, Method, USER_AGENT},
    ratelimiter::quota::Quota,
};
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::{
    common::{
        consts::{
            CONTRACT_PRODUCT_GROUP_SWAP_U, EP_ACCOUNT, EP_ACCURACY, EP_CANCEL_ORDER,
            EP_CONTRACT_INSTRUMENT, EP_CREATE_ORDER, EP_CURRENCY_PAIRS, EP_DEPTH, EP_TRADES,
            LBANK_SPOT_HTTP_URL, SIGNATURE_METHOD_HMAC,
        },
        credential::Credential,
        parse::instrument_id_from_lbank_symbol,
    },
    http::{
        error::{Error, Result},
        models::{
            LBankAccount, LBankAccuracy, LBankCancelOrderResult, LBankContractInstrument,
            LBankCreateOrderRequest, LBankCreateOrderResult, LBankDepth, LBankResponse,
            LBankRestTrade,
        },
        parse::{
            instrument_from_accuracy, instrument_from_contract, parse_depth_snapshot,
            parse_rest_trade_tick,
        },
    },
};

/// Default LBank REST rate limit (~16 req/s, from CCXT `rateLimit = 60` ms/token).
pub static LBANK_REST_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(16).expect("non-zero")).expect("valid quota")
});

/// HTTP client for LBank v2 spot.
#[derive(Debug, Clone)]
pub struct LbankHttpClient {
    client: HttpClient,
    credential: Option<Credential>,
    base_url: String,
}

impl LbankHttpClient {
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
            Some(*LBANK_REST_QUOTA),
            timeout_secs,
            proxy_url,
        )?;
        Ok(Self {
            client,
            credential,
            base_url: LBANK_SPOT_HTTP_URL.to_string(),
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

    fn default_headers() -> std::collections::HashMap<String, String> {
        std::collections::HashMap::from([(
            USER_AGENT.to_string(),
            NAUTILUS_USER_AGENT.to_string(),
        )])
    }

    /// Deserializes an LBank envelope, mapping `error_code != 0` to [`Error::Api`].
    fn unwrap_envelope<T: DeserializeOwned>(status: u16, body: &[u8]) -> Result<T> {
        if !(200..300).contains(&status) {
            return Err(Error::from_http_status(status, body));
        }
        let envelope: LBankResponse<T> = serde_json::from_slice(body)?;
        if envelope.error_code != 0 {
            return Err(Error::api(envelope.error_code, envelope.msg));
        }
        envelope.data.ok_or(Error::MissingData)
    }

    /// Sends an unauthenticated GET (optional `query`) and unwraps the envelope's `data`.
    async fn get_public<T: DeserializeOwned>(&self, path: &str, query: &str) -> Result<T> {
        let url = if query.is_empty() {
            format!("{}{path}", self.base_url)
        } else {
            format!("{}{path}?{query}", self.base_url)
        };
        let resp = self
            .client
            .request(Method::GET, url, None, None, None, None, None)
            .await
            .map_err(Error::from_http_client)?;
        Self::unwrap_envelope(resp.status.as_u16(), &resp.body)
    }

    /// Sends a signed POST: all params (business + system + `sign`) travel in the
    /// x-www-form-urlencoded body; `timestamp`/`signature_method`/`echostr` are repeated as headers.
    ///
    /// The system params are generated ONCE so the signed string, the body, and the headers agree.
    async fn post_signed<T: DeserializeOwned>(
        &self,
        path: &str,
        business: BTreeMap<String, String>,
    ) -> Result<T> {
        let credential = self
            .credential
            .as_ref()
            .ok_or_else(|| Error::auth("no credentials configured for signed request"))?;
        if credential.is_rsa() {
            return Err(Error::auth(
                "RSA signing (secret len > 32) is not implemented; use an HmacSHA256 API secret",
            ));
        }

        let timestamp = (get_atomic_clock_realtime().get_time_ns().as_u64() / 1_000_000).to_string();
        let echostr = Self::gen_echostr();

        let body = Self::assemble_signed_body(business, credential, &echostr, &timestamp)?;

        let headers = std::collections::HashMap::from([
            (
                "Content-Type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            ),
            ("timestamp".to_string(), timestamp),
            (
                "signature_method".to_string(),
                SIGNATURE_METHOD_HMAC.to_string(),
            ),
            ("echostr".to_string(), echostr),
        ]);

        let url = format!("{}{path}", self.base_url);
        let resp = self
            .client
            .request(
                Method::POST,
                url,
                None,
                Some(headers),
                Some(body.into_bytes()),
                None,
                None,
            )
            .await
            .map_err(Error::from_http_client)?;
        Self::unwrap_envelope(resp.status.as_u16(), &resp.body)
    }

    /// Assembles the x-www-form-urlencoded body for a signed request, byte-faithful to CCXT
    /// `lbank.py::sign`:
    /// - The signature is computed over `business + api_key + echostr + signature_method + timestamp`
    ///   (ASCII-sorted, raw-joined → uppercase MD5 → HMAC-SHA256).
    /// - The BODY carries only `business + api_key + sign`. `echostr`/`signature_method`/`timestamp`
    ///   travel in HTTP headers ONLY (CCXT's `extend(...)` builds a fresh dict for the signed string
    ///   and does NOT persist those three into the body `query`).
    fn assemble_signed_body(
        business: BTreeMap<String, String>,
        credential: &Credential,
        echostr: &str,
        timestamp: &str,
    ) -> Result<String> {
        // The full param set that is signed (BTreeMap → ASCII-ascending key order).
        let mut signed = business;
        signed.insert("api_key".to_string(), credential.api_key().to_string());
        signed.insert("echostr".to_string(), echostr.to_string());
        signed.insert(
            "signature_method".to_string(),
            SIGNATURE_METHOD_HMAC.to_string(),
        );
        signed.insert("timestamp".to_string(), timestamp.to_string());

        let prepared = Credential::prepared_str(&signed);
        let sign = credential.sign(&prepared);

        // Body = business + api_key + sign (drop the header-only system fields).
        let mut body_params = signed;
        body_params.remove("echostr");
        body_params.remove("signature_method");
        body_params.remove("timestamp");
        body_params.insert("sign".to_string(), sign);

        serde_urlencoded::to_string(&body_params)
            .map_err(|e| Error::transport(format!("failed to encode signed body: {e}")))
    }

    /// Generates a 38-char alphanumeric `echostr` (CCXT `uuid22()+uuid16()`; length must be 30-40).
    fn gen_echostr() -> String {
        let mut s = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        s.truncate(38);
        s
    }

    // -- Public domain methods --------------------------------------------------------------------

    /// Fetches the list of tradable spot pairs (`currencyPairs.do`), e.g. `["btc_usdt", ...]`.
    ///
    /// # Errors
    ///
    /// Returns an error on transport/status/envelope failure.
    pub async fn request_currency_pairs(&self) -> Result<Vec<String>> {
        self.get_public(EP_CURRENCY_PAIRS, "").await
    }

    /// Fetches precision/filter rows for all pairs (`accuracy.do`).
    ///
    /// # Errors
    ///
    /// Returns an error on transport/status/envelope failure.
    pub async fn request_accuracy(&self) -> Result<Vec<LBankAccuracy>> {
        self.get_public(EP_ACCURACY, "").await
    }

    /// Fetches all tradable spot instruments = `currencyPairs.do` ∩ `accuracy.do` (precision from
    /// accuracy, membership from currencyPairs).
    ///
    /// # Errors
    ///
    /// Returns an error on transport/status/envelope failure.
    pub async fn request_instruments(&self) -> Result<Vec<InstrumentAny>> {
        let tradable: ahash::AHashSet<String> =
            self.request_currency_pairs().await?.into_iter().collect();
        let accuracy = self.request_accuracy().await?;
        let ts_init = self.clock_ns();
        let mut out = Vec::with_capacity(accuracy.len());
        for acc in &accuracy {
            // If currencyPairs came back non-empty, restrict to that tradable set.
            if !tradable.is_empty() && !tradable.contains(&acc.symbol) {
                continue;
            }
            match instrument_from_accuracy(acc, ts_init) {
                Ok(inst) => out.push(inst),
                Err(e) => log::debug!("Skipping LBank instrument '{}': {e}", acc.symbol),
            }
        }
        Ok(out)
    }

    /// Fetches CONTRACT (USDT-perp) instruments from `cfd/openApi/v1/pub/instrument?productGroup=SwapU`.
    /// The client's `base_url` must be set to the contract host ([`LBANK_CONTRACT_HTTP_URL`]).
    ///
    /// # Errors
    ///
    /// Returns an error on transport/status/envelope failure.
    pub async fn request_contract_instruments(&self) -> Result<Vec<InstrumentAny>> {
        let query = format!("productGroup={CONTRACT_PRODUCT_GROUP_SWAP_U}");
        let rows: Vec<LBankContractInstrument> =
            self.get_public(EP_CONTRACT_INSTRUMENT, &query).await?;
        let ts_init = self.clock_ns();
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            match instrument_from_contract(row, ts_init) {
                Ok(inst) => out.push(inst),
                Err(e) => {
                    log::debug!("Skipping LBank contract instrument '{}': {e}", row.symbol);
                }
            }
        }
        Ok(out)
    }

    /// Fetches recent trades for `symbol` (`btc_usdt`) via `supplement/trades.do`.
    ///
    /// # Errors
    ///
    /// Returns an error on transport/status/envelope failure.
    pub async fn request_trades(
        &self,
        symbol: &str,
        size: u32,
        price_precision: u8,
        size_precision: u8,
    ) -> Result<Vec<TradeTick>> {
        let query = format!("symbol={symbol}&size={size}");
        let trades: Vec<LBankRestTrade> = self.get_public(EP_TRADES, &query).await?;
        let instrument_id = instrument_id_from_lbank_symbol(symbol);
        let ts_init = self.clock_ns();
        let mut out = Vec::with_capacity(trades.len());
        for t in &trades {
            match parse_rest_trade_tick(t, instrument_id, price_precision, size_precision, ts_init) {
                Ok(tick) => out.push(tick),
                Err(e) => log::debug!("Skipping LBank trade for {symbol}: {e}"),
            }
        }
        Ok(out)
    }

    /// Fetches an order book snapshot for `symbol` (`depth.do`).
    ///
    /// # Errors
    ///
    /// Returns an error on transport/status/envelope/parse failure.
    pub async fn request_order_book_snapshot(
        &self,
        symbol: &str,
        size: u32,
        price_precision: u8,
        size_precision: u8,
    ) -> Result<nautilus_model::data::OrderBookDeltas> {
        let query = format!("symbol={symbol}&size={size}");
        let depth: LBankDepth = self.get_public(EP_DEPTH, &query).await?;
        let instrument_id = instrument_id_from_lbank_symbol(symbol);
        let ts_init = self.clock_ns();
        parse_depth_snapshot(
            &depth,
            instrument_id,
            price_precision,
            size_precision,
            ts_init,
            ts_init,
        )
        .map_err(|e| Error::transport(e.to_string()))
    }

    // -- Signed domain methods --------------------------------------------------------------------

    /// Places a spot order (`supplement/create_order.do`, signed).
    ///
    /// # Errors
    ///
    /// Returns an error when unauthenticated or on a transport/status/envelope failure.
    pub async fn create_order(
        &self,
        request: &LBankCreateOrderRequest,
    ) -> Result<LBankCreateOrderResult> {
        let mut params = BTreeMap::new();
        params.insert("symbol".to_string(), request.symbol.clone());
        params.insert("type".to_string(), request.order_type.clone());
        params.insert("price".to_string(), request.price.clone());
        params.insert("amount".to_string(), request.amount.clone());
        if let Some(cid) = &request.custom_id {
            params.insert("custom_id".to_string(), cid.clone());
        }
        self.post_signed(EP_CREATE_ORDER, params).await
    }

    /// Cancels a spot order by venue id (`supplement/cancel_order.do`, signed).
    ///
    /// # Errors
    ///
    /// Returns an error when unauthenticated or on a transport/status/envelope failure.
    pub async fn cancel_order(
        &self,
        symbol: &str,
        order_id: &str,
    ) -> Result<LBankCancelOrderResult> {
        let mut params = BTreeMap::new();
        params.insert("symbol".to_string(), symbol.to_string());
        params.insert("orderId".to_string(), order_id.to_string());
        self.post_signed(EP_CANCEL_ORDER, params).await
    }

    /// Fetches spot account balances (`supplement/user_info_account.do`, signed).
    ///
    /// # Errors
    ///
    /// Returns an error when unauthenticated or on a transport/status/envelope failure.
    pub async fn request_account(&self) -> Result<LBankAccount> {
        self.post_signed(EP_ACCOUNT, BTreeMap::new()).await
    }

    fn clock_ns(&self) -> UnixNanos {
        get_atomic_clock_realtime().get_time_ns()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echostr_is_38_alphanumeric() {
        let e = LbankHttpClient::gen_echostr();
        assert_eq!(e.len(), 38);
        assert!(e.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn signed_body_matches_ccxt_layout() {
        // Same fixed inputs as the credential signing golden vector; the `sign` value below is the
        // one asserted there. The body must be keysort(business + api_key + sign) urlencoded, and
        // must NOT contain echostr / signature_method / timestamp (those are header-only per CCXT).
        let cred = Credential::new("testkey123".to_string(), "testsecret_short".to_string());
        let mut business = BTreeMap::new();
        for (k, v) in [
            ("symbol", "btc_usdt"),
            ("type", "buy"),
            ("price", "50000"),
            ("amount", "0.001"),
        ] {
            business.insert(k.to_string(), v.to_string());
        }
        let echostr = "Ab12Cd34Ef56Gh78Ij90Kl12Mn34Op5678";
        let body = LbankHttpClient::assemble_signed_body(
            business,
            &cred,
            echostr,
            "1700000000000",
        )
        .unwrap();

        let expected = "amount=0.001\
            &api_key=testkey123\
            &price=50000\
            &sign=b724e45c2c543cd931984d5757d50133dfaa56f6d4573302282461a2169ba83f\
            &symbol=btc_usdt\
            &type=buy";
        assert_eq!(body, expected);
        assert!(!body.contains("echostr"));
        assert!(!body.contains("signature_method"));
        assert!(!body.contains("timestamp"));
    }

    #[test]
    fn public_client_is_unauthenticated() {
        let c = LbankHttpClient::new(Some(5), None).unwrap();
        assert!(!c.is_authenticated());
    }

    #[test]
    fn envelope_error_code_maps_to_api_error() {
        // HTTP 200 but error_code != 0 must NOT be treated as success.
        let body = br#"{"result":"false","error_code":10007,"msg":"invalid sign"}"#;
        let res: Result<Vec<String>> = LbankHttpClient::unwrap_envelope(200, body);
        match res {
            Err(Error::Api { code, .. }) => assert_eq!(code, 10007),
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[test]
    fn envelope_success_returns_data() {
        let body = br#"{"result":"true","error_code":0,"data":["btc_usdt","eth_usdt"]}"#;
        let res: Vec<String> = LbankHttpClient::unwrap_envelope(200, body).unwrap();
        assert_eq!(res, vec!["btc_usdt", "eth_usdt"]);
    }

    #[test]
    fn envelope_non_2xx_is_status_error() {
        let res: Result<Vec<String>> = LbankHttpClient::unwrap_envelope(503, b"upstream down");
        assert!(matches!(res, Err(Error::Status { status: 503, .. })));
    }
}
