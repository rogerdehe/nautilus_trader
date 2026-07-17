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

//! Configuration for the KuCoin data and execution clients.

use nautilus_model::identifiers::{AccountId, TraderId};
use serde::{Deserialize, Serialize};

use crate::common::credential::credential_env_vars;

/// Configuration for the KuCoin live market-data client.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct KuCoinDataClientConfig {
    /// API key (falls back to `KUCOIN_API_KEY`). Optional for the public data path.
    pub api_key: Option<String>,
    /// API secret (falls back to `KUCOIN_API_SECRET`).
    pub api_secret: Option<String>,
    /// API passphrase (falls back to `KUCOIN_API_PASSPHRASE`).
    pub api_passphrase: Option<String>,
    /// Optional REST base URL override (defaults to `https://api.kucoin.com`).
    pub base_url_http: Option<String>,
}

impl KuCoinDataClientConfig {
    /// Creates a new default [`KuCoinDataClientConfig`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if a full API key/secret/passphrase triple resolves (config or env).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        resolve_triple(
            &self.api_key,
            &self.api_secret,
            &self.api_passphrase,
        )
        .is_some()
    }
}

/// Configuration for the KuCoin live execution client.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KuCoinExecClientConfig {
    /// Trader ID owning the account.
    pub trader_id: TraderId,
    /// Account ID (e.g. `KUCOIN-001`).
    pub account_id: AccountId,
    /// API key (falls back to `KUCOIN_API_KEY`).
    pub api_key: Option<String>,
    /// API secret (falls back to `KUCOIN_API_SECRET`).
    pub api_secret: Option<String>,
    /// API passphrase (falls back to `KUCOIN_API_PASSPHRASE`).
    pub api_passphrase: Option<String>,
    /// Optional REST base URL override (defaults to `https://api.kucoin.com`).
    pub base_url_http: Option<String>,
}

impl KuCoinExecClientConfig {
    /// Creates a new [`KuCoinExecClientConfig`] with the given trader/account IDs.
    #[must_use]
    pub fn new(trader_id: TraderId, account_id: AccountId) -> Self {
        Self {
            trader_id,
            account_id,
            api_key: None,
            api_secret: None,
            api_passphrase: None,
            base_url_http: None,
        }
    }

    /// Returns `true` if a full API key/secret/passphrase triple resolves (config or env).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        resolve_triple(&self.api_key, &self.api_secret, &self.api_passphrase).is_some()
    }
}

/// Resolves `(key, secret, passphrase)` from the provided values or environment variables.
fn resolve_triple(
    api_key: &Option<String>,
    api_secret: &Option<String>,
    api_passphrase: &Option<String>,
) -> Option<(String, String, String)> {
    use nautilus_core::env::get_or_env_var_opt;
    let (k_var, s_var, p_var) = credential_env_vars();
    let k = get_or_env_var_opt(api_key.clone(), k_var)?;
    let s = get_or_env_var_opt(api_secret.clone(), s_var)?;
    let p = get_or_env_var_opt(api_passphrase.clone(), p_var)?;
    Some((k, s, p))
}
