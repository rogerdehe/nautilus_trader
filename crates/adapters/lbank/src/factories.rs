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

//! Factory functions for creating LBank clients and components.

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
    identifiers::{AccountId, ClientId, TraderId},
};

use crate::{
    common::consts::{LBANK, lbank_venue},
    config::{LbankDataClientConfig, LbankExecClientConfig},
    contract_data::LbankContractDataClient,
    data::LbankDataClient,
    execution::LbankExecutionClient,
};

impl ClientConfig for LbankDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for LbankExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating LBank data clients.
#[derive(Debug, Clone, Default)]
pub struct LbankDataClientFactory;

impl LbankDataClientFactory {
    /// Creates a new [`LbankDataClientFactory`].
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl DataClientFactory for LbankDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let lbank_config = config
            .as_any()
            .downcast_ref::<LbankDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for LbankDataClientFactory. Expected LbankDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        // Route to the contract (USDT-perp) data client when the product selects futures.
        if lbank_config.is_contract() {
            let client = LbankContractDataClient::new(client_id, lbank_config)?;
            return Ok(Box::new(client));
        }
        let client = LbankDataClient::new(client_id, lbank_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        LBANK
    }

    fn config_type(&self) -> &'static str {
        "LbankDataClientConfig"
    }
}

/// Factory for creating LBank execution clients.
///
/// Spot is a `Cash` account with `Netting` OMS. Other account types are rejected.
#[derive(Debug, Clone)]
pub struct LbankExecutionClientFactory {
    trader_id: TraderId,
    account_id: AccountId,
}

impl LbankExecutionClientFactory {
    /// Creates a new [`LbankExecutionClientFactory`].
    #[must_use]
    pub const fn new(trader_id: TraderId, account_id: AccountId) -> Self {
        Self {
            trader_id,
            account_id,
        }
    }
}

impl ExecutionClientFactory for LbankExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let lbank_config = config
            .as_any()
            .downcast_ref::<LbankExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for LbankExecutionClientFactory. Expected LbankExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        let core = ExecutionClientCore::new(
            self.trader_id,
            ClientId::from(name),
            lbank_venue(),
            OmsType::Netting,
            self.account_id,
            AccountType::Cash,
            None,
            cache,
        );

        let client = LbankExecutionClient::new(core, lbank_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        LBANK
    }

    fn config_type(&self) -> &'static str {
        "LbankExecClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        clock::TestClock,
        factories::{ClientConfig, DataClientFactory},
        live::runner::set_data_event_sender,
        messages::DataEvent,
    };

    use super::*;

    fn setup_data_env() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);
    }

    fn setup_exec_env() {
        use nautilus_common::{live::runner::replace_exec_event_sender, messages::ExecutionEvent};
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        replace_exec_event_sender(sender);
    }

    #[test]
    fn data_factory_metadata() {
        let factory = LbankDataClientFactory::new();
        assert_eq!(factory.name(), LBANK);
        assert_eq!(factory.config_type(), "LbankDataClientConfig");
    }

    #[test]
    fn data_config_implements_client_config() {
        let config = LbankDataClientConfig::default();
        let boxed: Box<dyn ClientConfig> = Box::new(config);
        assert!(boxed.as_any().downcast_ref::<LbankDataClientConfig>().is_some());
    }

    #[test]
    fn data_factory_creates_client() {
        setup_data_env();
        let factory = LbankDataClientFactory::new();
        let config = LbankDataClientConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let client = factory
            .create("LBANK-TEST", &config, cache.into(), clock)
            .expect("factory should build data client");
        assert_eq!(client.client_id(), ClientId::from("LBANK-TEST"));
    }

    #[test]
    fn data_factory_rejects_wrong_config() {
        setup_data_env();
        let factory = LbankDataClientFactory::new();
        let wrong = LbankExecClientConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let err = match factory.create("LBANK-TEST", &wrong, cache.into(), clock) {
            Ok(_) => panic!("wrong config type should be rejected"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("LbankDataClientFactory"), "was: {err}");
        assert!(err.contains("LbankDataClientConfig"), "was: {err}");
    }

    #[test]
    fn exec_factory_metadata() {
        let factory = LbankExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("LBANK-001"),
        );
        assert_eq!(factory.name(), LBANK);
        assert_eq!(factory.config_type(), "LbankExecClientConfig");
    }

    #[test]
    fn exec_factory_creates_cash_client() {
        setup_exec_env();
        let factory = LbankExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("LBANK-001"),
        );
        let config = LbankExecClientConfig {
            api_key: Some("test-key".to_string()),
            api_secret: Some("test-secret".to_string()),
            ..LbankExecClientConfig::default()
        };
        let cache = Rc::new(RefCell::new(Cache::default()));
        let client = factory
            .create("LBANK-TEST", &config, cache.into())
            .expect("factory should build exec client");
        assert_eq!(client.client_id(), ClientId::from("LBANK-TEST"));
        assert_eq!(client.account_id(), AccountId::from("LBANK-001"));
        assert_eq!(client.venue(), lbank_venue());
        assert_eq!(client.oms_type(), OmsType::Netting);
    }

    #[test]
    fn exec_factory_rejects_missing_credentials() {
        setup_exec_env();
        let factory = LbankExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("LBANK-001"),
        );
        let config = LbankExecClientConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let err = match factory.create("LBANK-TEST", &config, cache.into()) {
            Ok(_) => panic!("missing credentials should be rejected"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("credentials"), "was: {err}");
    }
}
