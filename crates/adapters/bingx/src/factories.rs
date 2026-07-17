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

//! Factory functions for creating BingX clients and components.

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
    common::consts::{BINGX, bingx_venue},
    config::{BingXDataClientConfig, BingXExecClientConfig},
    data::BingXDataClient,
    execution::BingXExecutionClient,
};

impl ClientConfig for BingXDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for BingXExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating BingX data clients.
#[derive(Debug, Clone)]
pub struct BingXDataClientFactory;

impl BingXDataClientFactory {
    /// Creates a new [`BingXDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for BingXDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for BingXDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let config = config
            .as_any()
            .downcast_ref::<BingXDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BingXDataClientFactory. Expected BingXDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client = BingXDataClient::new(ClientId::from(name), config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        BINGX
    }

    fn config_type(&self) -> &'static str {
        "BingXDataClientConfig"
    }
}

/// Factory for creating BingX execution clients.
#[derive(Debug, Clone)]
pub struct BingXExecutionClientFactory;

impl BingXExecutionClientFactory {
    /// Creates a new [`BingXExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for BingXExecutionClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionClientFactory for BingXExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let config = config
            .as_any()
            .downcast_ref::<BingXExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BingXExecutionClientFactory. Expected BingXExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        // Spot trades cash; swap trades on margin.
        let (account_type, oms_type) = if config.product_type.is_spot() {
            (AccountType::Cash, OmsType::Hedging)
        } else {
            (AccountType::Margin, OmsType::Netting)
        };

        let core = ExecutionClientCore::new(
            config.trader_id,
            ClientId::from(name),
            bingx_venue(),
            oms_type,
            config.account_id,
            account_type,
            None, // base_currency
            cache,
        );

        let client = BingXExecutionClient::new(core, config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        BINGX
    }

    fn config_type(&self) -> &'static str {
        "BingXExecClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, factories::ExecutionClientFactory};
    use nautilus_model::identifiers::{AccountId, TraderId};
    use rstest::rstest;

    use super::*;
    use crate::common::enums::BingXProductType;

    #[rstest]
    fn data_factory_metadata() {
        let factory = BingXDataClientFactory::new();
        assert_eq!(factory.name(), BINGX);
        assert_eq!(factory.config_type(), "BingXDataClientConfig");
    }

    #[rstest]
    fn exec_factory_creates_spot_client() {
        let factory = BingXExecutionClientFactory::new();
        let config = BingXExecClientConfig {
            trader_id: TraderId::from("TRADER-001"),
            account_id: AccountId::from("BINGX-001"),
            product_type: BingXProductType::Spot,
            api_key: Some("k".to_string()),
            api_secret: Some("s".to_string()),
            ..Default::default()
        };
        let cache = Rc::new(RefCell::new(Cache::default()));
        let client = factory.create("BINGX-TEST", &config, cache.into()).unwrap();
        assert_eq!(client.client_id(), ClientId::from("BINGX-TEST"));
        assert_eq!(client.oms_type(), OmsType::Hedging);
    }

    #[rstest]
    fn exec_factory_rejects_wrong_config() {
        let factory = BingXExecutionClientFactory::new();
        let wrong = BingXDataClientConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let result = factory.create("BINGX-TEST", &wrong, cache.into());
        assert!(result.is_err());
    }
}
