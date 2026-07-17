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

//! KuCoin API credential storage and request signing.
//!
//! Signing is implemented first-hand from the CCXT reference (`ccxt/python/ccxt/kucoin.py::sign`)
//! for the `KC-API-KEY-VERSION: 2` scheme (the only one KuCoin currently issues):
//! 1. `timestamp` = current epoch milliseconds (as a decimal string).
//! 2. `passphrase` header = `base64(HMAC_SHA256(key=secret, msg=api_passphrase))`.
//! 3. `payload` = `timestamp + method + endpoint + body` where `endpoint` is the request path
//!    INCLUDING the leading `/api/v1/...` and any `?query` string (GET/DELETE) and `body` is the
//!    raw JSON body (POST) or empty.
//! 4. `KC-API-SIGN` = `base64(HMAC_SHA256(key=secret, msg=payload))`.
//!
//! Headers sent on every signed request: `KC-API-KEY`, `KC-API-SIGN`, `KC-API-TIMESTAMP`,
//! `KC-API-PASSPHRASE`, `KC-API-KEY-VERSION: 2`.

use std::fmt::Debug;

use aws_lc_rs::hmac;
use base64::prelude::*;
use nautilus_core::env::get_or_env_var_opt;
use zeroize::ZeroizeOnDrop;

const REDACTED: &str = "<redacted>";

/// The API-key version used for signing (KuCoin v2 keys).
pub const KC_API_KEY_VERSION: &str = "2";

/// Returns the `(key, secret, passphrase)` environment variable names for KuCoin credentials.
#[must_use]
pub fn credential_env_vars() -> (&'static str, &'static str, &'static str) {
    (
        "KUCOIN_API_KEY",
        "KUCOIN_API_SECRET",
        "KUCOIN_API_PASSPHRASE",
    )
}

/// KuCoin API credentials for signing requests. The secret and passphrase are zeroized on drop.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Credential {
    api_key: Box<str>,
    api_secret: Box<[u8]>,
    api_passphrase: Box<[u8]>,
}

impl Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Credential))
            .field("api_key", &self.api_key)
            .field("api_secret", &REDACTED)
            .field("api_passphrase", &REDACTED)
            .finish()
    }
}

impl Credential {
    /// Creates a new [`Credential`] from raw key/secret/passphrase strings.
    #[must_use]
    pub fn new(api_key: String, api_secret: String, api_passphrase: String) -> Self {
        Self {
            api_key: api_key.into_boxed_str(),
            api_secret: api_secret.into_bytes().into_boxed_slice(),
            api_passphrase: api_passphrase.into_bytes().into_boxed_slice(),
        }
    }

    /// Resolves credentials from the provided values or the `KUCOIN_API_KEY`/`KUCOIN_API_SECRET`/
    /// `KUCOIN_API_PASSPHRASE` environment variables. Returns `None` if any is missing.
    #[must_use]
    pub fn resolve(
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
    ) -> Option<Self> {
        let (key_var, secret_var, passphrase_var) = credential_env_vars();
        let key = get_or_env_var_opt(api_key, key_var);
        let secret = get_or_env_var_opt(api_secret, secret_var);
        let passphrase = get_or_env_var_opt(api_passphrase, passphrase_var);
        match (key, secret, passphrase) {
            (Some(k), Some(s), Some(p)) => Some(Self::new(k, s, p)),
            _ => None,
        }
    }

    /// Returns the API key.
    #[must_use]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Computes the signed passphrase header value = `base64(HMAC_SHA256(secret, api_passphrase))`.
    #[must_use]
    pub fn sign_passphrase(&self) -> String {
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret);
        let tag = hmac::sign(&key, &self.api_passphrase);
        BASE64_STANDARD.encode(tag.as_ref())
    }

    /// Computes `KC-API-SIGN` = `base64(HMAC_SHA256(secret, timestamp + method + endpoint + body))`.
    ///
    /// `endpoint` must include the leading path (`/api/v1/...`) and any `?query` string;
    /// `body` is the raw JSON body for POST requests, or empty for GET/DELETE.
    #[must_use]
    pub fn sign(&self, timestamp: &str, method: &str, endpoint: &str, body: &str) -> String {
        let mut message =
            Vec::with_capacity(timestamp.len() + method.len() + endpoint.len() + body.len());
        message.extend_from_slice(timestamp.as_bytes());
        message.extend_from_slice(method.as_bytes());
        message.extend_from_slice(endpoint.as_bytes());
        message.extend_from_slice(body.as_bytes());
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret);
        let tag = hmac::sign(&key, &message);
        BASE64_STANDARD.encode(tag.as_ref())
    }

    /// Returns a masked version of the API key for logging purposes.
    #[must_use]
    pub fn api_key_masked(&self) -> String {
        nautilus_core::string::secret::mask_api_key(&self.api_key)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // Golden vector cross-generated from the CCXT `kucoin.sign` v2 algorithm with fixed inputs:
    //   key="testkey123" secret="testsecret_short" passphrase="testpass" timestamp="1700000000000"
    const KEY: &str = "testkey123";
    const SECRET: &str = "testsecret_short";
    const PASSPHRASE: &str = "testpass";
    const TS: &str = "1700000000000";

    #[rstest]
    fn signing_matches_ccxt_golden_vector_get() {
        let cred = Credential::new(KEY.to_string(), SECRET.to_string(), PASSPHRASE.to_string());
        assert_eq!(cred.sign_passphrase(), "30Se1Vj59vpL2hGMq9dZqcAIZm8rLxhBpDyJY7Hfb1c=");
        let sign = cred.sign(
            TS,
            "GET",
            "/api/v1/market/orderbook/level2_20?symbol=BTC-USDT",
            "",
        );
        assert_eq!(sign, "olO9rLRX3sXqDqq9Yj/w+3ZJIqmsZgFcytBDBbOIsiE=");
    }

    #[rstest]
    fn signing_matches_ccxt_golden_vector_post() {
        let cred = Credential::new(KEY.to_string(), SECRET.to_string(), PASSPHRASE.to_string());
        let body =
            r#"{"clientOid":"abc","side":"buy","symbol":"BTC-USDT","type":"limit","price":"30000","size":"0.01"}"#;
        let sign = cred.sign(TS, "POST", "/api/v1/orders", body);
        assert_eq!(sign, "Kf5bX4u1B5JRXfTS5x0sMOqv8a0q2Vb/i2uVIvflWN4=");
    }

    #[rstest]
    fn test_debug_redacts_secrets() {
        let cred = Credential::new(KEY.to_string(), SECRET.to_string(), PASSPHRASE.to_string());
        let dbg_out = format!("{cred:?}");
        assert!(dbg_out.contains("api_secret: \"<redacted>\""));
        assert!(dbg_out.contains("api_passphrase: \"<redacted>\""));
        assert!(!dbg_out.contains(SECRET));
        assert!(!dbg_out.contains(PASSPHRASE));
    }

    #[rstest]
    fn test_resolve_partial_returns_none() {
        let (_, _, pass_var) = credential_env_vars();
        if std::env::var(pass_var).is_ok() {
            return;
        }
        let result = Credential::resolve(
            Some("my_key".to_string()),
            Some("my_secret".to_string()),
            None,
        );
        assert!(result.is_none());
    }
}
