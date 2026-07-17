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

//! Configuration structures for the Gate adapter.

use serde::{Deserialize, Serialize};

use crate::common::consts::{GATE_HTTP_BASE_URL, GATE_SPOT_WS_URL};

/// Configuration for the Gate data client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
pub struct GateDataClientConfig {
    /// API key (falls back to `GATE_API_KEY`). Not required for public market data.
    pub api_key: Option<String>,
    /// API secret (falls back to `GATE_API_SECRET`). Not required for public market data.
    pub api_secret: Option<String>,
    /// Override for the REST base host.
    pub base_url_http: Option<String>,
    /// Override for the spot WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Optional proxy URL for HTTP transport.
    pub proxy_url: Option<String>,
    /// HTTP timeout in seconds.
    #[builder(default = 10)]
    pub http_timeout_secs: u64,
    /// Interval for refreshing instruments in minutes.
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
}

impl Default for GateDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl GateDataClientConfig {
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
            .unwrap_or_else(|| GATE_HTTP_BASE_URL.to_string())
    }

    /// Returns the spot WebSocket URL, honoring any override.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| GATE_SPOT_WS_URL.to_string())
    }
}

/// Configuration for the Gate execution client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
pub struct GateExecClientConfig {
    /// API key (falls back to `GATE_API_KEY`).
    pub api_key: Option<String>,
    /// API secret (falls back to `GATE_API_SECRET`).
    pub api_secret: Option<String>,
    /// Override for the REST base host.
    pub base_url_http: Option<String>,
    /// Override for the spot WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Optional proxy URL for HTTP transport.
    pub proxy_url: Option<String>,
    /// HTTP timeout in seconds.
    #[builder(default = 10)]
    pub http_timeout_secs: u64,
    /// Maximum number of retry attempts for HTTP requests.
    #[builder(default = 3)]
    pub max_retries: u32,
}

impl Default for GateExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl GateExecClientConfig {
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
            .unwrap_or_else(|| GATE_HTTP_BASE_URL.to_string())
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
        let c = GateDataClientConfig::default();
        assert_eq!(c.http_timeout_secs, 10);
        assert_eq!(c.update_instruments_interval_mins, 60);
        assert!(!c.has_credentials());
        assert!(c.http_url().contains("gateio.ws"));
        assert!(c.ws_url().contains("ws/v4"));
    }

    #[test]
    fn data_config_has_credentials() {
        let c = GateDataClientConfig {
            api_key: Some("k".to_string()),
            api_secret: Some("s".to_string()),
            ..Default::default()
        };
        assert!(c.has_credentials());
    }

    #[test]
    fn data_config_blank_credentials_rejected() {
        let c = GateDataClientConfig {
            api_key: Some("  ".to_string()),
            api_secret: Some("s".to_string()),
            ..Default::default()
        };
        assert!(!c.has_credentials());
    }

    #[test]
    fn exec_config_defaults() {
        let c = GateExecClientConfig::default();
        assert_eq!(c.http_timeout_secs, 10);
        assert_eq!(c.max_retries, 3);
    }
}
