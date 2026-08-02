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

//! Configuration structures for the LBank adapter.

use serde::{Deserialize, Serialize};

use crate::common::consts::{
    LBANK_CONTRACT_HTTP_URL, LBANK_CONTRACT_WS_V3_URL, LBANK_SPOT_HTTP_URL, LBANK_SPOT_WS_URL,
};

/// Configuration for the LBank data client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
pub struct LbankDataClientConfig {
    /// API key (falls back to `LBANK_API_KEY`). Not required for public market data.
    pub api_key: Option<String>,
    /// API secret (falls back to `LBANK_API_SECRET`). Not required for public market data.
    pub api_secret: Option<String>,
    /// Override for the REST base host.
    pub base_url_http: Option<String>,
    /// Override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Optional proxy URL for HTTP transport.
    pub proxy_url: Option<String>,
    /// Product: `spot` (default) or `perp_linear`/`perp`/`swap`/`contract` for USDT-perp futures.
    /// Selects the spot vs contract data client + endpoints.
    pub product: Option<String>,
    /// HTTP timeout in seconds.
    #[builder(default = 10)]
    pub http_timeout_secs: u64,
    /// Interval for refreshing instruments in minutes.
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
}

impl Default for LbankDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl LbankDataClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when both credentials are populated and non-empty.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        non_empty(&self.api_key) && non_empty(&self.api_secret)
    }

    /// Returns the REST base host, honoring any override.
    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| LBANK_SPOT_HTTP_URL.to_string())
    }

    /// Returns the spot WebSocket URL, honoring any override.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| LBANK_SPOT_WS_URL.to_string())
    }

    /// `true` when the product selects USDT-perp contract futures (vs spot).
    #[must_use]
    pub fn is_contract(&self) -> bool {
        matches!(
            self.product.as_deref().map(str::trim),
            Some("perp_linear" | "perp" | "swap" | "contract" | "futures" | "perpetual")
        )
    }

    /// Returns the CONTRACT REST base host, honoring any override.
    #[must_use]
    pub fn contract_http_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| LBANK_CONTRACT_HTTP_URL.to_string())
    }

    /// Returns the CONTRACT v3 market-data WebSocket URL, honoring any override.
    #[must_use]
    pub fn contract_ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| LBANK_CONTRACT_WS_V3_URL.to_string())
    }
}

/// Configuration for the LBank execution client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
pub struct LbankExecClientConfig {
    /// API key (falls back to `LBANK_API_KEY`).
    pub api_key: Option<String>,
    /// API secret (falls back to `LBANK_API_SECRET`).
    pub api_secret: Option<String>,
    /// Override for the REST base host.
    pub base_url_http: Option<String>,
    /// Optional proxy URL for HTTP transport.
    pub proxy_url: Option<String>,
    /// HTTP timeout in seconds.
    #[builder(default = 10)]
    pub http_timeout_secs: u64,
    /// Maximum number of retry attempts for HTTP requests.
    #[builder(default = 3)]
    pub max_retries: u32,
}

impl Default for LbankExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl LbankExecClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when both credentials are populated and non-empty.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        non_empty(&self.api_key) && non_empty(&self.api_secret)
    }

    /// Returns the REST base host, honoring any override.
    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| LBANK_SPOT_HTTP_URL.to_string())
    }
}

fn non_empty(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_config_defaults() {
        let c = LbankDataClientConfig::default();
        assert_eq!(c.http_timeout_secs, 10);
        assert_eq!(c.update_instruments_interval_mins, 60);
        assert!(!c.has_credentials());
        assert!(c.http_url().contains("lbkex"));
        assert!(c.ws_url().contains("ws/V2"));
    }

    #[test]
    fn data_config_has_credentials() {
        let c = LbankDataClientConfig {
            api_key: Some("k".to_string()),
            api_secret: Some("s".to_string()),
            ..Default::default()
        };
        assert!(c.has_credentials());
    }

    #[test]
    fn data_config_blank_credentials_rejected() {
        let c = LbankDataClientConfig {
            api_key: Some("  ".to_string()),
            api_secret: Some("s".to_string()),
            ..Default::default()
        };
        assert!(!c.has_credentials());
    }

    #[test]
    fn exec_config_defaults() {
        let c = LbankExecClientConfig::default();
        assert_eq!(c.http_timeout_secs, 10);
        assert_eq!(c.max_retries, 3);
    }
}
