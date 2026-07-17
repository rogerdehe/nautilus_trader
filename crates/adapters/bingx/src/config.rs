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

//! Configuration structures for the BingX adapter.

use nautilus_model::identifiers::{AccountId, TraderId};
use serde::{Deserialize, Serialize};

use crate::common::{
    consts::{BINGX_HTTP_BASE_URL, BINGX_HTTP_TIMEOUT_SECS, BINGX_WS_SPOT_URL, BINGX_WS_SWAP_URL},
    credential::credential_env_vars,
    enums::BingXProductType,
};

/// Configuration for the BingX data client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BingXDataClientConfig {
    /// Optional API key for authenticated endpoints (public data needs none).
    pub api_key: Option<String>,
    /// Optional API secret for authenticated endpoints.
    pub api_secret: Option<String>,
    /// Product (market) type this client serves.
    pub product_type: BingXProductType,
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the public WebSocket URL.
    pub base_url_ws: Option<String>,
    /// HTTP timeout in seconds.
    pub http_timeout_secs: u64,
    /// Interval for refreshing instruments in minutes (0 disables).
    pub update_instruments_interval_mins: u64,
}

impl Default for BingXDataClientConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_secret: None,
            product_type: BingXProductType::Spot,
            base_url_http: None,
            base_url_ws: None,
            http_timeout_secs: BINGX_HTTP_TIMEOUT_SECS,
            update_instruments_interval_mins: 60,
        }
    }
}

impl BingXDataClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when API credentials are available (in config or env vars).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var) = credential_env_vars();
        let has_key = self.api_key.is_some() || std::env::var(key_var).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_var).is_ok();
        has_key && has_secret
    }

    /// Returns the effective HTTP base URL.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| BINGX_HTTP_BASE_URL.to_string())
    }

    /// Returns the effective public WebSocket URL for the configured product.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws.clone().unwrap_or_else(|| {
            match self.product_type {
                BingXProductType::Spot => BINGX_WS_SPOT_URL,
                BingXProductType::Swap => BINGX_WS_SWAP_URL,
            }
            .to_string()
        })
    }
}

/// Configuration for the BingX execution client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BingXExecClientConfig {
    /// The trader identifier.
    pub trader_id: TraderId,
    /// The account identifier.
    pub account_id: AccountId,
    /// Optional API key.
    pub api_key: Option<String>,
    /// Optional API secret.
    pub api_secret: Option<String>,
    /// Product (market) type this client trades.
    pub product_type: BingXProductType,
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// HTTP timeout in seconds.
    pub http_timeout_secs: u64,
}

impl Default for BingXExecClientConfig {
    fn default() -> Self {
        Self {
            trader_id: TraderId::from("TRADER-000"),
            account_id: AccountId::from("BINGX-001"),
            api_key: None,
            api_secret: None,
            product_type: BingXProductType::Spot,
            base_url_http: None,
            http_timeout_secs: BINGX_HTTP_TIMEOUT_SECS,
        }
    }
}

impl BingXExecClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when API credentials are available (in config or env vars).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var) = credential_env_vars();
        let has_key = self.api_key.is_some() || std::env::var(key_var).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_var).is_ok();
        has_key && has_secret
    }

    /// Returns the effective HTTP base URL.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| BINGX_HTTP_BASE_URL.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_config_defaults() {
        let cfg = BingXDataClientConfig::new();
        assert_eq!(cfg.product_type, BingXProductType::Spot);
        assert_eq!(cfg.http_base_url(), BINGX_HTTP_BASE_URL);
        assert_eq!(cfg.ws_url(), BINGX_WS_SPOT_URL);
    }

    #[test]
    fn swap_config_uses_swap_ws() {
        let cfg = BingXDataClientConfig {
            product_type: BingXProductType::Swap,
            ..Default::default()
        };
        assert_eq!(cfg.ws_url(), BINGX_WS_SWAP_URL);
    }

    #[test]
    fn exec_config_defaults() {
        let cfg = BingXExecClientConfig::new();
        assert_eq!(cfg.account_id, AccountId::from("BINGX-001"));
    }
}
