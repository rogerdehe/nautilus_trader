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

//! Live execution client for HashKey Global (spot).
//!
//! Provides the [`ExecutionClient`] trait surface backed by [`ExecutionClientCore`]. Account-state
//! emission is wired through the shared [`ExecutionEventEmitter`]. Order submission / cancellation
//! and reconciliation report generation (which require the HashKey private user-data WebSocket
//! stream via a listen key) are the remaining follow-up on top of this scaffold.

use async_trait::async_trait;
use nautilus_common::{clients::ExecutionClient, live::runner::get_exec_event_sender};
use nautilus_core::{
    UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::OmsType,
    identifiers::{AccountId, ClientId, Venue},
    types::{AccountBalance, MarginBalance},
};

use crate::{common::consts::hashkey_venue, config::HashKeyExecClientConfig, http::HashKeyHttpClient};

/// A live execution client for the HashKey Global exchange (spot).
pub struct HashKeyExecutionClient {
    core: ExecutionClientCore,
    #[allow(dead_code)]
    config: HashKeyExecClientConfig,
    emitter: ExecutionEventEmitter,
    #[allow(dead_code)]
    http_client: HashKeyHttpClient,
    #[allow(dead_code)]
    clock: &'static AtomicTime,
}

impl std::fmt::Debug for HashKeyExecutionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(HashKeyExecutionClient))
            .field("client_id", &self.core.client_id)
            .field("account_id", &self.core.account_id)
            .finish_non_exhaustive()
    }
}

impl HashKeyExecutionClient {
    /// Creates a new [`HashKeyExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be constructed.
    pub fn new(core: ExecutionClientCore, config: HashKeyExecClientConfig) -> anyhow::Result<Self> {
        let http_client = HashKeyHttpClient::with_credentials(
            config.api_key.clone(),
            config.api_secret.clone(),
            Some(config.http_base_url()),
            Some(config.http_timeout_secs),
        )?;

        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            core.account_type,
            None,
        );

        Ok(Self {
            core,
            config,
            emitter,
            http_client,
            clock,
        })
    }
}

#[async_trait(?Send)]
impl ExecutionClient for HashKeyExecutionClient {
    fn is_connected(&self) -> bool {
        self.core.is_connected()
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        hashkey_venue()
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account_owned(&self.core.account_id)
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        self.emitter
            .emit_account_state(balances, margins, reported, ts_event);
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.core.is_started() {
            return Ok(());
        }
        self.emitter.set_sender(get_exec_event_sender());
        self.core.set_started();
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.core.set_stopped();
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }
        self.core.set_connected();
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            return Ok(());
        }
        self.core.set_disconnected();
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }
}
