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

//! Configuration structures for the HashKey adapter.

use nautilus_model::identifiers::{AccountId, TraderId};
use nautilus_network::websocket::TransportBackend;
use serde::{Deserialize, Serialize};

use crate::common::{
    consts::{
        HASHKEY_HTTP_TESTNET_URL, HASHKEY_HTTP_URL, HASHKEY_WS_PRIVATE_TESTNET_URL,
        HASHKEY_WS_PRIVATE_URL, HASHKEY_WS_PUBLIC_TESTNET_URL, HASHKEY_WS_PUBLIC_URL,
    },
    credential::credential_env_vars,
};

/// Configuration for the HashKey data client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
pub struct HashKeyDataClientConfig {
    /// Optional API key for authenticated endpoints (listen key / private data).
    pub api_key: Option<String>,
    /// Optional API secret for authenticated endpoints.
    pub api_secret: Option<String>,
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the public WebSocket URL.
    pub base_url_ws_public: Option<String>,
    /// Whether to target the HashKey simulation (testnet) environment.
    #[builder(default)]
    pub testnet: bool,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Interval for refreshing instruments in minutes.
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
    /// WebSocket transport backend.
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for HashKeyDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl HashKeyDataClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when both API credentials are available (config or env vars).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var) = credential_env_vars();
        let has_key = self.api_key.is_some() || std::env::var(key_var).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_var).is_ok();
        has_key && has_secret
    }

    /// Returns the HTTP base URL, honouring overrides and the testnet flag.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http.clone().unwrap_or_else(|| {
            if self.testnet {
                HASHKEY_HTTP_TESTNET_URL.to_string()
            } else {
                HASHKEY_HTTP_URL.to_string()
            }
        })
    }

    /// Returns the public WebSocket URL, honouring overrides and the testnet flag.
    #[must_use]
    pub fn ws_public_url(&self) -> String {
        self.base_url_ws_public.clone().unwrap_or_else(|| {
            if self.testnet {
                HASHKEY_WS_PUBLIC_TESTNET_URL.to_string()
            } else {
                HASHKEY_WS_PUBLIC_URL.to_string()
            }
        })
    }
}

/// Configuration for the HashKey execution client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
pub struct HashKeyExecClientConfig {
    /// The trader ID for the client.
    #[builder(default = TraderId::from("TRADER-001"))]
    pub trader_id: TraderId,
    /// The account ID for the client.
    #[builder(default = AccountId::from("HASHKEY-001"))]
    pub account_id: AccountId,
    /// Optional API key for authenticated endpoints.
    pub api_key: Option<String>,
    /// Optional API secret for authenticated endpoints.
    pub api_secret: Option<String>,
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the private WebSocket URL.
    pub base_url_ws_private: Option<String>,
    /// Whether to target the HashKey simulation (testnet) environment.
    #[builder(default)]
    pub testnet: bool,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// WebSocket transport backend.
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for HashKeyExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl HashKeyExecClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when both API credentials are available (config or env vars).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var) = credential_env_vars();
        let has_key = self.api_key.is_some() || std::env::var(key_var).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_var).is_ok();
        has_key && has_secret
    }

    /// Returns the HTTP base URL, honouring overrides and the testnet flag.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http.clone().unwrap_or_else(|| {
            if self.testnet {
                HASHKEY_HTTP_TESTNET_URL.to_string()
            } else {
                HASHKEY_HTTP_URL.to_string()
            }
        })
    }

    /// Returns the private WebSocket URL, honouring overrides and the testnet flag.
    #[must_use]
    pub fn ws_private_url(&self) -> String {
        self.base_url_ws_private.clone().unwrap_or_else(|| {
            if self.testnet {
                HASHKEY_WS_PRIVATE_TESTNET_URL.to_string()
            } else {
                HASHKEY_WS_PRIVATE_URL.to_string()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn data_config_defaults() {
        let config = HashKeyDataClientConfig::default();
        assert_eq!(config.http_base_url(), HASHKEY_HTTP_URL);
        assert_eq!(config.ws_public_url(), HASHKEY_WS_PUBLIC_URL);
        assert_eq!(config.http_timeout_secs, 60);
        assert!(!config.testnet);
    }

    #[rstest]
    fn data_config_testnet_urls() {
        let config = HashKeyDataClientConfig::builder().testnet(true).build();
        assert_eq!(config.http_base_url(), HASHKEY_HTTP_TESTNET_URL);
        assert_eq!(config.ws_public_url(), HASHKEY_WS_PUBLIC_TESTNET_URL);
    }

    #[rstest]
    fn data_config_toml_roundtrip() {
        let config: HashKeyDataClientConfig = toml::from_str(
            r#"
testnet = true
http_timeout_secs = 90
"#,
        )
        .unwrap();
        assert!(config.testnet);
        assert_eq!(config.http_timeout_secs, 90);
    }

    #[rstest]
    fn exec_config_defaults() {
        let config = HashKeyExecClientConfig::default();
        assert_eq!(config.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(config.account_id, AccountId::from("HASHKEY-001"));
        assert_eq!(config.ws_private_url(), HASHKEY_WS_PRIVATE_URL);
    }
}
