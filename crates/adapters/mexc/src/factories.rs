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

//! Factory functions for creating MEXC clients and components.

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
    common::consts::{MEXC, MEXC_VENUE},
    config::{MexcDataClientConfig, MexcExecClientConfig},
    data::MexcDataClient,
    execution::MexcExecutionClient,
};

impl ClientConfig for MexcDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for MexcExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating MEXC data clients.
#[derive(Debug, Clone, Default)]
pub struct MexcDataClientFactory;

impl MexcDataClientFactory {
    /// Creates a new [`MexcDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl DataClientFactory for MexcDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let config = config
            .as_any()
            .downcast_ref::<MexcDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for MexcDataClientFactory. Expected MexcDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client = MexcDataClient::new(ClientId::from(name), config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        MEXC
    }

    fn config_type(&self) -> &'static str {
        "MexcDataClientConfig"
    }
}

/// Factory for creating MEXC execution clients.
#[derive(Debug, Clone, Default)]
pub struct MexcExecutionClientFactory;

impl MexcExecutionClientFactory {
    /// Creates a new [`MexcExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ExecutionClientFactory for MexcExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let config = config
            .as_any()
            .downcast_ref::<MexcExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for MexcExecutionClientFactory. Expected MexcExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        // MEXC spot is a cash account with netting OMS (no separate long/short positions).
        let core = ExecutionClientCore::new(
            config.trader_id,
            ClientId::from(name),
            *MEXC_VENUE,
            OmsType::Netting,
            config.account_id,
            AccountType::Cash,
            None, // base_currency
            cache,
        );

        let client = MexcExecutionClient::new(core, config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        MEXC
    }

    fn config_type(&self) -> &'static str {
        "MexcExecClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        factories::{ClientConfig, ExecutionClientFactory},
    };
    use nautilus_model::identifiers::{AccountId, TraderId};
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_data_factory_metadata() {
        let factory = MexcDataClientFactory::new();
        assert_eq!(factory.name(), MEXC);
        assert_eq!(factory.config_type(), "MexcDataClientConfig");
    }

    #[rstest]
    fn test_exec_factory_metadata() {
        let factory = MexcExecutionClientFactory::new();
        assert_eq!(factory.name(), MEXC);
        assert_eq!(factory.config_type(), "MexcExecClientConfig");
    }

    #[rstest]
    fn test_exec_config_is_client_config() {
        let config = MexcExecClientConfig::builder()
            .trader_id(TraderId::from("TRADER-001"))
            .account_id(AccountId::from("MEXC-001"))
            .build();
        let boxed: Box<dyn ClientConfig> = Box::new(config);
        assert!(boxed.as_any().downcast_ref::<MexcExecClientConfig>().is_some());
    }

    #[rstest]
    fn test_exec_factory_creates_client() {
        let factory = MexcExecutionClientFactory::new();
        let config = MexcExecClientConfig::builder()
            .trader_id(TraderId::from("TRADER-001"))
            .account_id(AccountId::from("MEXC-001"))
            .api_key("test_key".to_string())
            .api_secret("test_secret".to_string())
            .build();

        let cache = Rc::new(RefCell::new(Cache::default()));
        let client = factory.create("MEXC-TEST", &config, cache.into()).unwrap();
        assert_eq!(client.client_id(), ClientId::from("MEXC-TEST"));
    }

    #[rstest]
    fn test_exec_factory_rejects_wrong_config() {
        let factory = MexcExecutionClientFactory::new();
        let wrong = MexcDataClientConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let result = factory.create("MEXC-TEST", &wrong, cache.into());
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("Invalid config type"));
    }
}
