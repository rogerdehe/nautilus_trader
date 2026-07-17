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

//! Bitget API credential storage and request signing helpers.
//!
//! Ported first-hand from CCXT `ccxt/python/ccxt/bitget.py::sign()`:
//! `ACCESS-SIGN = base64(HMAC_SHA256(secret, timestamp + method + requestPath + body))`
//! where `requestPath` = `/api` + path (+ `?` + sorted raw query for signed GETs), and `body`
//! is the compact JSON string for POSTs. Headers: `ACCESS-KEY`, `ACCESS-SIGN`,
//! `ACCESS-TIMESTAMP` (millisecond epoch), `ACCESS-PASSPHRASE`.

use std::fmt::Debug;

use aws_lc_rs::hmac;
use base64::prelude::*;
use nautilus_core::{env::get_or_env_var_opt, string::secret::REDACTED};
use zeroize::ZeroizeOnDrop;

/// Returns the environment variable names for API credentials.
#[must_use]
pub fn credential_env_vars() -> (&'static str, &'static str, &'static str) {
    ("BITGET_API_KEY", "BITGET_API_SECRET", "BITGET_API_PASSPHRASE")
}

/// Bitget API credentials for signing requests.
///
/// Uses HMAC SHA256 (base64) for request signing as per Bitget API specifications.
/// Secrets are automatically zeroized on drop for security.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Credential {
    api_key: Box<str>,
    api_passphrase: Box<str>,
    api_secret: Box<[u8]>,
}

impl Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Credential))
            .field("api_key", &self.api_key)
            .field("api_passphrase", &REDACTED)
            .field("api_secret", &REDACTED)
            .finish()
    }
}

impl Credential {
    /// Creates a new [`Credential`] instance.
    #[must_use]
    pub fn new(api_key: String, api_secret: String, api_passphrase: String) -> Self {
        Self {
            api_key: api_key.into_boxed_str(),
            api_passphrase: api_passphrase.into_boxed_str(),
            api_secret: api_secret.into_bytes().into_boxed_slice(),
        }
    }

    /// Resolves credentials from provided values or environment variables.
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

    /// Returns the API passphrase.
    #[must_use]
    pub fn api_passphrase(&self) -> &str {
        &self.api_passphrase
    }

    /// Signs a request message according to the Bitget authentication scheme.
    ///
    /// `request_path` must include the leading `/api` prefix and, for signed GETs, the
    /// pre-sorted `?query` string (raw, not percent-encoded). `body` is the compact JSON
    /// string for POSTs (empty for GETs).
    pub fn sign(&self, timestamp: &str, method: &str, request_path: &str, body: &str) -> String {
        self.sign_bytes(timestamp, method, request_path, Some(body.as_bytes()))
    }

    /// Signs a request message using raw body bytes to avoid any UTF-8 conversion or
    /// re-serialization differences between the signed content and the bytes sent.
    pub fn sign_bytes(
        &self,
        timestamp: &str,
        method: &str,
        request_path: &str,
        body: Option<&[u8]>,
    ) -> String {
        let mut message = Vec::with_capacity(
            timestamp.len() + method.len() + request_path.len() + body.map_or(0, |b| b.len()),
        );
        message.extend_from_slice(timestamp.as_bytes());
        message.extend_from_slice(method.as_bytes());
        message.extend_from_slice(request_path.as_bytes());

        if let Some(b) = body {
            message.extend_from_slice(b);
        }

        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret[..]);
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

    // Golden vectors generated by CCXT's real `bitget.sign()` (ccxt-master) with
    // nonce fixed to 1700000000000 and timeDifference=0. See task golden-vector step.
    const API_KEY: &str = "testkey";
    const API_SECRET: &str = "testsecret";
    const API_PASSPHRASE: &str = "testpass";
    const TS: &str = "1700000000000";

    fn cred() -> Credential {
        Credential::new(
            API_KEY.to_string(),
            API_SECRET.to_string(),
            API_PASSPHRASE.to_string(),
        )
    }

    #[rstest]
    fn signing_matches_ccxt_golden_get_no_params() {
        let sig = cred().sign(TS, "GET", "/api/v2/spot/account/assets", "");
        assert_eq!(sig, "Q1NgfFPae7KSSO+mytgk12HJC2D6G1htSKF/yr9g1yA=");
    }

    #[rstest]
    fn signing_matches_ccxt_golden_get_with_params() {
        // Sorted raw query: granularity=1min&limit=100&symbol=BTCUSDT
        let sig = cred().sign(
            TS,
            "GET",
            "/api/v2/spot/market/candles?granularity=1min&limit=100&symbol=BTCUSDT",
            "",
        );
        assert_eq!(sig, "AL9exAZnInV6E/c53/ipvzH5cARnTT7BQuQ2kqCdzG8=");
    }

    #[rstest]
    fn signing_matches_ccxt_golden_post_json_body() {
        let body = r#"{"symbol":"BTCUSDT","side":"buy","orderType":"limit","force":"gtc","price":"20000","size":"0.001"}"#;
        let sig = cred().sign(TS, "POST", "/api/v2/spot/trade/place-order", body);
        assert_eq!(sig, "9BHAnIsrMTyFkm4MIIQCSUDCVUZG9r+HHC4JCfN/HWM=");
    }

    #[rstest]
    fn test_debug_redacts_secrets() {
        let dbg_out = format!("{:?}", cred());
        assert!(dbg_out.contains("api_secret: \"<redacted>\""));
        assert!(dbg_out.contains("api_passphrase: \"<redacted>\""));
        assert!(!dbg_out.contains(API_SECRET));
    }

    #[rstest]
    fn test_resolve_with_all_args() {
        let result = Credential::resolve(
            Some("my_key".to_string()),
            Some("my_secret".to_string()),
            Some("my_pass".to_string()),
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().api_key(), "my_key");
    }

    #[rstest]
    fn test_resolve_with_partial_args_returns_none() {
        let (_, _, passphrase_var) = credential_env_vars();
        if std::env::var(passphrase_var).is_ok() {
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
