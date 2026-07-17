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

//! Configuration structures for the Bitget adapter.

use nautilus_model::identifiers::{AccountId, TraderId};
use serde::{Deserialize, Serialize};

use crate::common::{consts::BITGET_HTTP_URL, credential::credential_env_vars};

/// Configuration for the Bitget data client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
pub struct BitgetDataClientConfig {
    /// Optional API key for authenticated endpoints.
    pub api_key: Option<String>,
    /// Optional API secret for authenticated endpoints.
    pub api_secret: Option<String>,
    /// Optional API passphrase for authenticated endpoints.
    pub api_passphrase: Option<String>,
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the public WebSocket URL.
    pub base_url_ws_public: Option<String>,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
}

impl Default for BitgetDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl BitgetDataClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when all API credential fields are available (in config or env vars).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        has_credentials(&self.api_key, &self.api_secret, &self.api_passphrase)
    }

    /// Returns the HTTP base URL, falling back to the venue default when unset.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| BITGET_HTTP_URL.to_string())
    }
}

/// Configuration for the Bitget execution client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
pub struct BitgetExecClientConfig {
    /// The trader ID for the client.
    #[builder(default = TraderId::from("TRADER-001"))]
    pub trader_id: TraderId,
    /// The account ID for the client.
    #[builder(default = AccountId::from("BITGET-001"))]
    pub account_id: AccountId,
    /// Optional API key for authenticated endpoints.
    pub api_key: Option<String>,
    /// Optional API secret for authenticated endpoints.
    pub api_secret: Option<String>,
    /// Optional API passphrase for authenticated endpoints.
    pub api_passphrase: Option<String>,
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
}

impl Default for BitgetExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl BitgetExecClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when all API credential fields are available (in config or env vars).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        has_credentials(&self.api_key, &self.api_secret, &self.api_passphrase)
    }

    /// Returns the HTTP base URL, falling back to the venue default when unset.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| BITGET_HTTP_URL.to_string())
    }
}

fn has_credentials(
    api_key: &Option<String>,
    api_secret: &Option<String>,
    api_passphrase: &Option<String>,
) -> bool {
    let (key_var, secret_var, passphrase_var) = credential_env_vars();
    let has_key = api_key.is_some() || std::env::var(key_var).is_ok();
    let has_secret = api_secret.is_some() || std::env::var(secret_var).is_ok();
    let has_passphrase = api_passphrase.is_some() || std::env::var(passphrase_var).is_ok();
    has_key && has_secret && has_passphrase
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_data_config_defaults() {
        let config = BitgetDataClientConfig::default();
        assert_eq!(config.http_timeout_secs, 60);
        assert_eq!(config.http_base_url(), BITGET_HTTP_URL);
    }

    #[rstest]
    fn test_data_config_toml() {
        let config: BitgetDataClientConfig = toml::from_str("http_timeout_secs = 90").unwrap();
        assert_eq!(config.http_timeout_secs, 90);
    }

    #[rstest]
    fn test_exec_config_defaults() {
        let config = BitgetExecClientConfig::default();
        assert_eq!(config.account_id, AccountId::from("BITGET-001"));
    }
}
