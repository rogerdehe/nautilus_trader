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

//! Gate (gate.io) APIv4 credential storage and request signing.
//!
//! Signing is implemented first-hand from the CCXT reference (`ccxt/python/ccxt/gate.py::sign`):
//! 1. `bodyHash = hex(SHA512(body_payload))` where `body_payload` is the JSON body (empty string
//!    for GET/DELETE requests). Empty body hashes to the well-known `cf83e135…da3e`.
//! 2. `timestamp` is UNIX SECONDS (CCXT does `parse_to_int(nonce_ms / 1000)`), NOT milliseconds.
//! 3. `signaturePath = "/api/v4" + entirePath` where `entirePath = "/" + type + "/" + path`
//!    (e.g. `/api/v4/spot/orders`).
//! 4. `payload = METHOD "\n" signaturePath "\n" rawQueryString "\n" bodyHash "\n" timestamp`
//!    joined by literal `\n`. `rawQueryString` is the RAW (non-url-encoded) query — it feeds the
//!    signature while the URL carries the url-encoded form (they coincide when values have no
//!    special chars).
//! 5. `signature = hex(HMAC_SHA512(secret, payload))`.
//! 6. Headers: `KEY`, `Timestamp`, `SIGN`, `Content-Type: application/json`.

use std::fmt::Debug;

use aws_lc_rs::{digest, hmac};
use nautilus_core::env::get_or_env_var_opt;
use zeroize::ZeroizeOnDrop;

const REDACTED: &str = "<redacted>";

/// Returns the `(key, secret)` environment variable names for Gate credentials.
#[must_use]
pub fn credential_env_vars() -> (&'static str, &'static str) {
    ("GATE_API_KEY", "GATE_API_SECRET")
}

/// Gate API credentials for signing requests. The secret is zeroized on drop.
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

    /// Resolves credentials from the provided values or the `GATE_API_KEY`/`GATE_API_SECRET`
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

    /// Computes `hex(SHA512(body))` for a request body payload (empty string for GET/DELETE).
    #[must_use]
    pub fn body_hash(body: &str) -> String {
        let d = digest::digest(&digest::SHA512, body.as_bytes());
        hex::encode(d.as_ref())
    }

    /// Builds the newline-joined string-to-sign per Gate APIv4.
    ///
    /// * `method` — uppercased HTTP method (`GET`, `POST`, `DELETE`, ...).
    /// * `signature_path` — `/api/v4` + entirePath (e.g. `/api/v4/spot/orders`).
    /// * `raw_query` — raw (non-url-encoded) query string (empty when none).
    /// * `body` — JSON body (empty string when none).
    /// * `timestamp` — UNIX seconds as a string.
    #[must_use]
    pub fn payload(
        method: &str,
        signature_path: &str,
        raw_query: &str,
        body: &str,
        timestamp: &str,
    ) -> String {
        let body_hash = Self::body_hash(body);
        format!("{method}\n{signature_path}\n{raw_query}\n{body_hash}\n{timestamp}")
    }

    /// Computes `signature = hex(HMAC_SHA512(secret, payload))`.
    #[must_use]
    pub fn sign(&self, payload: &str) -> String {
        let key = hmac::Key::new(hmac::HMAC_SHA512, &self.api_secret);
        let tag = hmac::sign(&key, payload.as_bytes());
        hex::encode(tag.as_ref())
    }

    /// Convenience: builds the payload and returns the final signature.
    #[must_use]
    pub fn sign_request(
        &self,
        method: &str,
        signature_path: &str,
        raw_query: &str,
        body: &str,
        timestamp: &str,
    ) -> String {
        let payload = Self::payload(method, signature_path, raw_query, body, timestamp);
        self.sign(&payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "testkey123";
    const SECRET: &str = "testsecret_gate_v4";

    /// Golden vectors cross-generated from the CCXT `gate.sign` algorithm (fixed key/secret and
    /// `timestamp = 1700000000` seconds). Must reproduce CCXT's `signature` byte-for-byte.
    #[test]
    fn signing_matches_ccxt_golden_vector_get() {
        let cred = Credential::new(KEY.to_string(), SECRET.to_string());
        let sig = cred.sign_request(
            "GET",
            "/api/v4/spot/orders",
            "currency_pair=BTC_USDT&status=open",
            "",
            "1700000000",
        );
        assert_eq!(
            sig,
            "dfcc4c11ea12d795b54cbdb98d6bf689a708971970108ff4ee0c49a9381b9285fdcfa2308cc40f0752a5b1058219d85a08e2204e04ee1742649ef3c7b168df82"
        );
    }

    #[test]
    fn signing_matches_ccxt_golden_vector_post() {
        let cred = Credential::new(KEY.to_string(), SECRET.to_string());
        let body = r#"{"currency_pair":"BTC_USDT","side":"buy","amount":"0.001","price":"50000"}"#;
        let sig = cred.sign_request("POST", "/api/v4/spot/orders", "", body, "1700000000");
        assert_eq!(
            sig,
            "8fc3d72d0ed4ccd7e7433699aaf532b7694fecc2c5b7e61fa99aa67c384970803d9c67584f2ee7d897cee3c663eb2fec9f4f127bebf5b95fc3404b7ac226bbda"
        );
    }

    #[test]
    fn empty_body_hash_is_wellknown_sha512() {
        assert_eq!(
            Credential::body_hash(""),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
    }
}
