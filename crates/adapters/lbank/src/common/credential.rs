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

//! LBank API credential storage and request signing.
//!
//! Signing is implemented first-hand from the CCXT reference (`ccxt/python/ccxt/lbank.py::sign`):
//! 1. Collect business params + `api_key` + `echostr` + `signature_method` + `timestamp`.
//! 2. Sort by key (ASCII ascending) and join as `k=v&k=v` with RAW (non-url-encoded) values.
//! 3. `preparedStr = UPPERCASE(hex(MD5(joined)))`.
//! 4. `sign = hex(HMAC_SHA256(secret, preparedStr))` (lowercase). CCXT selects RSA when
//!    `len(secret) > 32`; here we implement the HmacSHA256 path (the common API-key case).
//! 5. All params incl. `sign` go in the request BODY (`application/x-www-form-urlencoded`); the
//!    exchange also echoes `timestamp`/`signature_method`/`echostr` as headers.

use std::{collections::BTreeMap, fmt::Debug};

use aws_lc_rs::hmac;
use md5::{Digest, Md5};
use nautilus_core::env::get_or_env_var_opt;
use zeroize::ZeroizeOnDrop;

const REDACTED: &str = "<redacted>";

/// Returns the `(key, secret)` environment variable names for LBank credentials.
#[must_use]
pub fn credential_env_vars() -> (&'static str, &'static str) {
    ("LBANK_API_KEY", "LBANK_API_SECRET")
}

/// LBank API credentials for signing requests. The secret is zeroized on drop.
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

    /// Resolves credentials from the provided values or the `LBANK_API_KEY`/`LBANK_API_SECRET`
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

    /// `true` when the secret length selects LBank's RSA signing (`len > 32`), which is NOT yet
    /// implemented by [`Self::sign`]. HmacSHA256 keys (the common case) return `false`.
    #[must_use]
    pub fn is_rsa(&self) -> bool {
        self.api_secret.len() > 32
    }

    /// Builds the `preparedStr` = `UPPERCASE(hex(MD5(sorted_join(params))))` for `params` (which
    /// must already include `api_key`/`echostr`/`signature_method`/`timestamp`).
    #[must_use]
    pub fn prepared_str(params: &BTreeMap<String, String>) -> String {
        // BTreeMap iterates in ASCII-ascending key order; join raw (non-url-encoded) values.
        let joined = params
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let mut hasher = Md5::new();
        hasher.update(joined.as_bytes());
        hex::encode(hasher.finalize()).to_uppercase()
    }

    /// Computes `sign = hex(HMAC_SHA256(secret, prepared_str))` (lowercase hex).
    #[must_use]
    pub fn sign(&self, prepared_str: &str) -> String {
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret);
        let tag = hmac::sign(&key, prepared_str.as_bytes());
        hex::encode(tag.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden vector cross-generated from the CCXT `lbank.sign` algorithm (see the adapter build
    /// notes): fixed key/secret/timestamp/echostr must reproduce CCXT's `preparedStr` + `sign`.
    #[test]
    fn signing_matches_ccxt_golden_vector() {
        let mut params: BTreeMap<String, String> = BTreeMap::new();
        for (k, v) in [
            ("symbol", "btc_usdt"),
            ("type", "buy"),
            ("price", "50000"),
            ("amount", "0.001"),
            ("api_key", "testkey123"),
            ("echostr", "Ab12Cd34Ef56Gh78Ij90Kl12Mn34Op5678"),
            ("signature_method", "HmacSHA256"),
            ("timestamp", "1700000000000"),
        ] {
            params.insert(k.to_string(), v.to_string());
        }

        let prepared = Credential::prepared_str(&params);
        assert_eq!(prepared, "BEC60B5E506F5D6E1AD5A0CCBBFC3C6C");

        let cred = Credential::new("testkey123".to_string(), "testsecret_short".to_string());
        assert!(!cred.is_rsa());
        assert_eq!(
            cred.sign(&prepared),
            "b724e45c2c543cd931984d5757d50133dfaa56f6d4573302282461a2169ba83f"
        );
    }
}
