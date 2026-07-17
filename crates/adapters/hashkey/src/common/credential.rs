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

//! HashKey Global API credential storage and request signing.
//!
//! Signing is implemented first-hand from the CCXT reference (`ccxt/python/ccxt/hashkey.py::sign`):
//! 1. Build `additionalParams = {timestamp[, recvWindow]}` then extend with the business params.
//!    The extend preserves INSERTION ORDER (not sorted) — this is the string-to-sign order.
//! 2. `stringToSign = custom_urlencode(totalParams)` where `custom_urlencode` is the standard
//!    form-urlencode with `%2C` restored to a literal comma (`,`).
//! 3. `signature = hex(HMAC_SHA256(secret, stringToSign))` (lowercase hex).
//! 4. For GET the signature is appended to the query string; for POST it goes in the body. The
//!    request carries headers `X-HK-APIKEY`, `INPUT-SOURCE` (broker id) and `broker_sign`
//!    (= the same signature).

use std::fmt::Debug;

use aws_lc_rs::hmac;
use nautilus_core::env::get_or_env_var_opt;
use zeroize::ZeroizeOnDrop;

const REDACTED: &str = "<redacted>";

/// Returns the `(key, secret)` environment variable names for HashKey credentials.
#[must_use]
pub fn credential_env_vars() -> (&'static str, &'static str) {
    ("HASHKEY_API_KEY", "HASHKEY_API_SECRET")
}

/// HashKey API credentials for signing requests. The secret is zeroized on drop.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Credential {
    api_key: Box<str>,
    api_secret: Box<[u8]>,
}

impl Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Credential))
            .field("api_key", &self.api_key)
            .field("api_secret", &REDACTED)
            .finish()
    }
}

impl Credential {
    /// Creates a new [`Credential`] from raw key/secret strings.
    #[must_use]
    pub fn new(api_key: String, api_secret: String) -> Self {
        Self {
            api_key: api_key.into_boxed_str(),
            api_secret: api_secret.into_bytes().into_boxed_slice(),
        }
    }

    /// Resolves credentials from the provided values or the `HASHKEY_API_KEY`/`HASHKEY_API_SECRET`
    /// environment variables. Returns `None` if either is missing.
    #[must_use]
    pub fn resolve(api_key: Option<String>, api_secret: Option<String>) -> Option<Self> {
        let (key_var, secret_var) = credential_env_vars();
        let key = get_or_env_var_opt(api_key, key_var);
        let secret = get_or_env_var_opt(api_secret, secret_var);
        match (key, secret) {
            (Some(k), Some(s)) => Some(Self::new(k, s)),
            _ => None,
        }
    }

    /// Returns the API key.
    #[must_use]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Computes `signature = hex(HMAC_SHA256(secret, string_to_sign))` (lowercase hex).
    ///
    /// `string_to_sign` must already be the ordered `custom_urlencode` of all signed params
    /// (see [`custom_urlencode`]).
    #[must_use]
    pub fn sign(&self, string_to_sign: &str) -> String {
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret);
        let tag = hmac::sign(&key, string_to_sign.as_bytes());
        hex::encode(tag.as_ref())
    }
}

/// Builds HashKey's `custom_urlencode(params)` string: standard form-urlencoding of the ordered
/// `(key, value)` pairs with `%2C` restored to a literal comma (matching CCXT `custom_urlencode`).
///
/// Order is significant — HashKey signs over the params in the exact order supplied, so callers must
/// place `timestamp` (and optional `recvWindow`) first, followed by the business params.
#[must_use]
pub fn custom_urlencode(params: &[(String, String)]) -> String {
    let encoded = serde_urlencoded::to_string(params).unwrap_or_default();
    encoded.replace("%2C", ",")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden vector cross-generated from the real CCXT `hashkey.sign` private-GET path
    /// (`ccxt.hashkey().custom_urlencode` + `hmac`) with fixed inputs. Byte-for-byte match required.
    #[test]
    fn signing_matches_ccxt_golden_vector() {
        // additionalParams (timestamp) first, then business params — insertion order preserved.
        let params: Vec<(String, String)> = vec![
            ("timestamp".to_string(), "1700000000000".to_string()),
            ("symbol".to_string(), "BTCUSDT".to_string()),
            ("side".to_string(), "BUY".to_string()),
            ("type".to_string(), "LIMIT".to_string()),
            ("quantity".to_string(), "0.001".to_string()),
            ("price".to_string(), "50000".to_string()),
        ];

        let string_to_sign = custom_urlencode(&params);
        assert_eq!(
            string_to_sign,
            "timestamp=1700000000000&symbol=BTCUSDT&side=BUY&type=LIMIT&quantity=0.001&price=50000"
        );

        let cred = Credential::new("testkey123".to_string(), "testsecret_short".to_string());
        assert_eq!(
            cred.sign(&string_to_sign),
            "2e6795389e72e860cac4e0536e0f329db259b74499c8e47f4273f676185a83fa"
        );
    }

    /// `custom_urlencode` keeps commas literal (CCXT replaces `%2C` back to `,`).
    #[test]
    fn custom_urlencode_preserves_commas() {
        let params = vec![
            ("timestamp".to_string(), "1700000000000".to_string()),
            ("orderIds".to_string(), "1,2,3".to_string()),
        ];
        assert_eq!(
            custom_urlencode(&params),
            "timestamp=1700000000000&orderIds=1,2,3"
        );
    }
}
