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

//! Factories for constructing KuCoin data and execution clients.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::CacheView,
    clients::{DataClient, ExecutionClient},
    clock::Clock,
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::ClientId,
};

use crate::{
    common::consts::{KUCOIN_VENUE, kucoin_venue},
    config::{KuCoinDataClientConfig, KuCoinExecClientConfig},
    data::KuCoinDataClient,
    execution::KuCoinExecutionClient,
};

impl ClientConfig for KuCoinDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for KuCoinExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating KuCoin data clients.
#[derive(Debug, Clone, Default)]
pub struct KuCoinDataClientFactory;

impl KuCoinDataClientFactory {
    /// Creates a new [`KuCoinDataClientFactory`].
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl DataClientFactory for KuCoinDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let cfg = config
            .as_any()
            .downcast_ref::<KuCoinDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for KuCoinDataClientFactory. Expected KuCoinDataClientConfig, was {config:?}"
                )
            })?
            .clone();
        let client = KuCoinDataClient::new(ClientId::from(name), cfg)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        KUCOIN_VENUE
    }

    fn config_type(&self) -> &'static str {
        "KuCoinDataClientConfig"
    }
}

/// Factory for creating KuCoin execution clients.
#[derive(Debug, Clone, Default)]
pub struct KuCoinExecClientFactory;

impl KuCoinExecClientFactory {
    /// Creates a new [`KuCoinExecClientFactory`].
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ExecutionClientFactory for KuCoinExecClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let cfg = config
            .as_any()
            .downcast_ref::<KuCoinExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for KuCoinExecClientFactory. Expected KuCoinExecClientConfig, was {config:?}"
                )
            })?
            .clone();

        // KuCoin spot is a cash account; the venue nets exposure per instrument.
        let core = ExecutionClientCore::new(
            cfg.trader_id,
            ClientId::from(name),
            kucoin_venue(),
            OmsType::Netting,
            cfg.account_id,
            AccountType::Cash,
            None, // base_currency
            cache,
        );
        let client = KuCoinExecutionClient::new(core, cfg)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        KUCOIN_VENUE
    }

    fn config_type(&self) -> &'static str {
        "KuCoinExecClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn data_factory_metadata() {
        let factory = KuCoinDataClientFactory::new();
        assert_eq!(factory.name(), "KUCOIN");
        assert_eq!(factory.config_type(), "KuCoinDataClientConfig");
    }

    #[rstest]
    fn exec_factory_metadata() {
        let factory = KuCoinExecClientFactory::new();
        assert_eq!(factory.name(), "KUCOIN");
        assert_eq!(factory.config_type(), "KuCoinExecClientConfig");
    }

    #[rstest]
    fn data_config_downcast_roundtrip() {
        // Proves config -> ClientConfig -> downcast wiring the factory relies on.
        let config = KuCoinDataClientConfig::new();
        let boxed: Box<dyn ClientConfig> = Box::new(config);
        assert!(boxed.as_any().downcast_ref::<KuCoinDataClientConfig>().is_some());
    }
}
