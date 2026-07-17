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

//! Factory functions for creating Bitget clients and components.

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
    common::consts::{BITGET, BITGET_VENUE},
    config::{BitgetDataClientConfig, BitgetExecClientConfig},
    data::BitgetDataClient,
    execution::BitgetExecutionClient,
};

impl ClientConfig for BitgetDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for BitgetExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Bitget data clients.
#[derive(Debug, Clone, Default)]
pub struct BitgetDataClientFactory;

impl BitgetDataClientFactory {
    /// Creates a new [`BitgetDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl DataClientFactory for BitgetDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let config = config
            .as_any()
            .downcast_ref::<BitgetDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BitgetDataClientFactory. Expected BitgetDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client = BitgetDataClient::new(ClientId::from(name), config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        BITGET
    }

    fn config_type(&self) -> &'static str {
        "BitgetDataClientConfig"
    }
}

/// Factory for creating Bitget execution clients.
#[derive(Debug, Clone, Default)]
pub struct BitgetExecutionClientFactory;

impl BitgetExecutionClientFactory {
    /// Creates a new [`BitgetExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ExecutionClientFactory for BitgetExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let config = config
            .as_any()
            .downcast_ref::<BitgetExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BitgetExecutionClientFactory. Expected BitgetExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        // Spot cash account with netting OMS.
        let core = ExecutionClientCore::new(
            config.trader_id,
            ClientId::from(name),
            *BITGET_VENUE,
            OmsType::Netting,
            config.account_id,
            AccountType::Cash,
            None, // base_currency
            cache,
        );

        let client = BitgetExecutionClient::new(core, config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        BITGET
    }

    fn config_type(&self) -> &'static str {
        "BitgetExecClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, factories::ExecutionClientFactory};
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_data_factory_name() {
        let factory = BitgetDataClientFactory::new();
        assert_eq!(factory.name(), BITGET);
        assert_eq!(factory.config_type(), "BitgetDataClientConfig");
    }

    #[rstest]
    fn test_exec_factory_creates_client() {
        let factory = BitgetExecutionClientFactory::new();
        let config = BitgetExecClientConfig {
            api_key: Some("k".to_string()),
            api_secret: Some("s".to_string()),
            api_passphrase: Some("p".to_string()),
            ..Default::default()
        };
        let cache = Rc::new(RefCell::new(Cache::default()));
        let client = factory.create("BITGET", &config, cache.into()).unwrap();
        assert_eq!(client.client_id(), ClientId::from("BITGET"));
    }

    #[rstest]
    fn test_exec_factory_rejects_wrong_config() {
        let factory = BitgetExecutionClientFactory::new();
        let wrong = BitgetDataClientConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let result = factory.create("BITGET", &wrong, cache.into());
        assert!(result.is_err());
    }
}
