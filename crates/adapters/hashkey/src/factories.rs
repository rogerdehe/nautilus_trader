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

//! Factory functions for creating HashKey clients and components.

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
    common::consts::{HASHKEY_VENUE, hashkey_venue},
    config::{HashKeyDataClientConfig, HashKeyExecClientConfig},
    data::HashKeyDataClient,
    execution::HashKeyExecutionClient,
};

impl ClientConfig for HashKeyDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for HashKeyExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating HashKey data clients.
#[derive(Debug, Clone)]
pub struct HashKeyDataClientFactory;

impl HashKeyDataClientFactory {
    /// Creates a new [`HashKeyDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for HashKeyDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for HashKeyDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let config = config
            .as_any()
            .downcast_ref::<HashKeyDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for HashKeyDataClientFactory. Expected HashKeyDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client = HashKeyDataClient::new(ClientId::from(name), config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        HASHKEY_VENUE
    }

    fn config_type(&self) -> &'static str {
        "HashKeyDataClientConfig"
    }
}

/// Factory for creating HashKey execution clients.
#[derive(Debug, Clone)]
pub struct HashKeyExecutionClientFactory;

impl HashKeyExecutionClientFactory {
    /// Creates a new [`HashKeyExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for HashKeyExecutionClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionClientFactory for HashKeyExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let config = config
            .as_any()
            .downcast_ref::<HashKeyExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for HashKeyExecutionClientFactory. Expected HashKeyExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        // HashKey spot is a cash account with netting OMS (no positions, single net balance).
        let core = ExecutionClientCore::new(
            config.trader_id,
            ClientId::from(name),
            hashkey_venue(),
            OmsType::Netting,
            config.account_id,
            AccountType::Cash,
            None, // base_currency
            cache,
        );

        let client = HashKeyExecutionClient::new(core, config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        HASHKEY_VENUE
    }

    fn config_type(&self) -> &'static str {
        "HashKeyExecClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        factories::{ClientConfig, ExecutionClientFactory},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn data_factory_metadata() {
        let factory = HashKeyDataClientFactory::new();
        assert_eq!(factory.name(), HASHKEY_VENUE);
        assert_eq!(factory.config_type(), "HashKeyDataClientConfig");
    }

    #[rstest]
    fn exec_config_implements_client_config() {
        let config = HashKeyExecClientConfig::default();
        let boxed: Box<dyn ClientConfig> = Box::new(config);
        assert!(boxed.as_any().downcast_ref::<HashKeyExecClientConfig>().is_some());
    }

    #[rstest]
    fn exec_factory_creates_client() {
        let factory = HashKeyExecutionClientFactory::new();
        let config = HashKeyExecClientConfig {
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            ..Default::default()
        };
        let cache = Rc::new(RefCell::new(Cache::default()));

        let client = factory.create("HASHKEY-TEST", &config, cache.into()).unwrap();
        assert_eq!(client.client_id(), ClientId::from("HASHKEY-TEST"));
    }

    #[rstest]
    fn exec_factory_rejects_wrong_config() {
        let factory = HashKeyExecutionClientFactory::new();
        let wrong = HashKeyDataClientConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create("HASHKEY-TEST", &wrong, cache.into());
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("Invalid config type"));
    }
}
