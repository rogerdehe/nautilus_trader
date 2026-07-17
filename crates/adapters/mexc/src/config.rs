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

//! Configuration structures for the MEXC adapter.

use nautilus_model::identifiers::{AccountId, TraderId};
use serde::{Deserialize, Serialize};

use crate::common::{consts::MEXC_HTTP_URL, credential::credential_env_vars};

/// Configuration for the MEXC spot data client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
pub struct MexcDataClientConfig {
    /// Optional API key for authenticated endpoints (data client only needs public access).
    pub api_key: Option<String>,
    /// Optional API secret for authenticated endpoints.
    pub api_secret: Option<String>,
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the spot WebSocket URL.
    pub base_url_ws: Option<String>,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Interval for refreshing instruments in minutes.
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
    /// Interval (seconds) for REST polling of trade data (WS market data is protobuf; see crate docs).
    #[builder(default = 1)]
    pub trades_poll_interval_secs: u64,
}

impl Default for MexcDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl MexcDataClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when both API credentials are available (in config or env vars).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var) = credential_env_vars();
        let has_key = self.api_key.is_some() || std::env::var(key_var).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_var).is_ok();
        has_key && has_secret
    }

    /// Returns the HTTP base URL, falling back to the MEXC default when unset.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| MEXC_HTTP_URL.to_string())
    }
}

/// Configuration for the MEXC spot execution client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
pub struct MexcExecClientConfig {
    /// The trader ID for the client.
    #[builder(default = TraderId::from("TRADER-001"))]
    pub trader_id: TraderId,
    /// The account ID for the client.
    #[builder(default = AccountId::from("MEXC-001"))]
    pub account_id: AccountId,
    /// Optional API key for authenticated endpoints.
    pub api_key: Option<String>,
    /// Optional API secret for authenticated endpoints.
    pub api_secret: Option<String>,
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
}

impl Default for MexcExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl MexcExecClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when both API credentials are available (in config or env vars).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var) = credential_env_vars();
        let has_key = self.api_key.is_some() || std::env::var(key_var).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_var).is_ok();
        has_key && has_secret
    }

    /// Returns the HTTP base URL, falling back to the MEXC default when unset.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| MEXC_HTTP_URL.to_string())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_data_config_defaults() {
        let config = MexcDataClientConfig::default();
        assert_eq!(config.http_timeout_secs, 60);
        assert_eq!(config.http_base_url(), "https://api.mexc.com");
        assert_eq!(config.update_instruments_interval_mins, 60);
    }

    #[rstest]
    fn test_data_config_toml() {
        let config: MexcDataClientConfig = toml::from_str(
            "
http_timeout_secs = 90
trades_poll_interval_secs = 2
",
        )
        .unwrap();
        assert_eq!(config.http_timeout_secs, 90);
        assert_eq!(config.trades_poll_interval_secs, 2);
    }

    #[rstest]
    fn test_exec_config_defaults() {
        let config = MexcExecClientConfig::default();
        assert_eq!(config.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(config.account_id, AccountId::from("MEXC-001"));
        assert_eq!(config.http_base_url(), "https://api.mexc.com");
    }

    #[rstest]
    fn test_exec_config_override_url() {
        let config = MexcExecClientConfig::builder()
            .base_url_http("https://custom.proxy".to_string())
            .build();
        assert_eq!(config.http_base_url(), "https://custom.proxy");
    }
}
