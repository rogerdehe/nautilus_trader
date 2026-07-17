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

//! Factory functions for creating Gate clients and components.

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
    common::consts::{GATE, gate_venue},
    config::{GateDataClientConfig, GateExecClientConfig},
    data::GateDataClient,
    execution::GateExecutionClient,
};

impl ClientConfig for GateDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for GateExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Gate data clients.
#[derive(Debug, Clone, Default)]
pub struct GateDataClientFactory;

impl GateDataClientFactory {
    /// Creates a new [`GateDataClientFactory`].
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl DataClientFactory for GateDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let gate_config = config
            .as_any()
            .downcast_ref::<GateDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for GateDataClientFactory. Expected GateDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        let client = GateDataClient::new(client_id, gate_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        GATE
    }

    fn config_type(&self) -> &'static str {
        "GateDataClientConfig"
    }
}

/// Factory for creating Gate execution clients.
///
/// Spot is a `Cash` account with `Netting` OMS. Other account types are rejected.
#[derive(Debug, Clone)]
pub struct GateExecutionClientFactory {
    trader_id: TraderId,
    account_id: AccountId,
}

impl GateExecutionClientFactory {
    /// Creates a new [`GateExecutionClientFactory`].
    #[must_use]
    pub const fn new(trader_id: TraderId, account_id: AccountId) -> Self {
        Self {
            trader_id,
            account_id,
        }
    }
}

impl ExecutionClientFactory for GateExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let gate_config = config
            .as_any()
            .downcast_ref::<GateExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for GateExecutionClientFactory. Expected GateExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        let core = ExecutionClientCore::new(
            self.trader_id,
            ClientId::from(name),
            gate_venue(),
            OmsType::Netting,
            self.account_id,
            AccountType::Cash,
            None,
            cache,
        );

        let client = GateExecutionClient::new(core, gate_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        GATE
    }

    fn config_type(&self) -> &'static str {
        "GateExecClientConfig"
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
        let factory = GateDataClientFactory::new();
        assert_eq!(factory.name(), GATE);
        assert_eq!(factory.config_type(), "GateDataClientConfig");
    }

    #[test]
    fn data_config_implements_client_config() {
        let config = GateDataClientConfig::default();
        let boxed: Box<dyn ClientConfig> = Box::new(config);
        assert!(boxed.as_any().downcast_ref::<GateDataClientConfig>().is_some());
    }

    #[test]
    fn data_factory_creates_client() {
        setup_data_env();
        let factory = GateDataClientFactory::new();
        let config = GateDataClientConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let client = factory
            .create("GATE-TEST", &config, cache.into(), clock)
            .expect("factory should build data client");
        assert_eq!(client.client_id(), ClientId::from("GATE-TEST"));
    }

    #[test]
    fn data_factory_rejects_wrong_config() {
        setup_data_env();
        let factory = GateDataClientFactory::new();
        let wrong = GateExecClientConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let err = match factory.create("GATE-TEST", &wrong, cache.into(), clock) {
            Ok(_) => panic!("wrong config type should be rejected"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("GateDataClientFactory"), "was: {err}");
        assert!(err.contains("GateDataClientConfig"), "was: {err}");
    }

    #[test]
    fn exec_factory_metadata() {
        let factory = GateExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("GATE-001"),
        );
        assert_eq!(factory.name(), GATE);
        assert_eq!(factory.config_type(), "GateExecClientConfig");
    }

    #[test]
    fn exec_factory_creates_cash_client() {
        setup_exec_env();
        let factory = GateExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("GATE-001"),
        );
        let config = GateExecClientConfig {
            api_key: Some("test-key".to_string()),
            api_secret: Some("test-secret".to_string()),
            ..GateExecClientConfig::default()
        };
        let cache = Rc::new(RefCell::new(Cache::default()));
        let client = factory
            .create("GATE-TEST", &config, cache.into())
            .expect("factory should build exec client");
        assert_eq!(client.client_id(), ClientId::from("GATE-TEST"));
        assert_eq!(client.account_id(), AccountId::from("GATE-001"));
        assert_eq!(client.venue(), gate_venue());
        assert_eq!(client.oms_type(), OmsType::Netting);
    }

    #[test]
    fn exec_factory_rejects_missing_credentials() {
        setup_exec_env();
        let factory = GateExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("GATE-001"),
        );
        let config = GateExecClientConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let err = match factory.create("GATE-TEST", &config, cache.into()) {
            Ok(_) => panic!("missing credentials should be rejected"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("credentials"), "was: {err}");
    }
}
