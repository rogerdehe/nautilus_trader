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

//! BingX live execution client.
//!
//! This is the structural skeleton: it wires credentials, the account/OMS identity, and account
//! state emission. Order submission/cancellation and order/fill reconciliation are staged as
//! follow-ups (see the adapter notes) — the signed HTTP order endpoints live in
//! [`crate::common::consts`] and signing is already proven by the credential golden-vector test.

use nautilus_common::messages::execution::{CancelOrder, SubmitOrder};
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::OmsType,
    identifiers::{AccountId, ClientId, Venue},
    types::{AccountBalance, MarginBalance},
};

use crate::{common::consts::bingx_venue, config::BingXExecClientConfig, http::client::BingXHttpClient};

/// Live execution client for BingX.
pub struct BingXExecutionClient {
    core: ExecutionClientCore,
    emitter: ExecutionEventEmitter,
    #[allow(dead_code)]
    http_client: BingXHttpClient,
    #[allow(dead_code)]
    config: BingXExecClientConfig,
}

impl std::fmt::Debug for BingXExecutionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BingXExecutionClient")
            .field("client_id", &self.core.client_id)
            .field("account_id", &self.core.account_id)
            .finish_non_exhaustive()
    }
}

impl BingXExecutionClient {
    /// Creates a new [`BingXExecutionClient`] from an execution core and config.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(
        core: ExecutionClientCore,
        config: BingXExecClientConfig,
    ) -> anyhow::Result<Self> {
        let http_client = if config.has_api_credentials() {
            BingXHttpClient::with_credentials(
                config.api_key.clone().unwrap_or_default(),
                config.api_secret.clone().unwrap_or_default(),
                Some(config.http_base_url()),
                Some(config.http_timeout_secs),
            )
        } else {
            BingXHttpClient::new(Some(config.http_base_url()), Some(config.http_timeout_secs))
        };

        let emitter = ExecutionEventEmitter::new(
            get_atomic_clock_realtime(),
            core.trader_id,
            core.account_id,
            core.account_type,
            core.base_currency,
        );

        Ok(Self {
            core,
            emitter,
            http_client,
            config,
        })
    }
}

#[async_trait::async_trait(?Send)]
impl nautilus_common::clients::ExecutionClient for BingXExecutionClient {
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
        bingx_venue()
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
        let sender = nautilus_common::live::runner::get_exec_event_sender();
        self.emitter.set_sender(sender);
        self.core.set_started();
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if !self.core.is_started() {
            return Ok(());
        }
        self.core.set_stopped();
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        // Balance sync + private order/fill stream are staged follow-ups; mark connected so the
        // engine treats the client as live for submit/cancel routing.
        self.core.set_connected();
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.core.set_disconnected();
        Ok(())
    }

    fn submit_order(&self, _cmd: SubmitOrder) -> anyhow::Result<()> {
        // TODO: sign + POST spot/v1/trade/order, then emit submitted/accepted/rejected events.
        anyhow::bail!("BingX submit_order not yet implemented")
    }

    fn cancel_order(&self, _cmd: CancelOrder) -> anyhow::Result<()> {
        // TODO: sign + POST spot/v1/trade/cancel, then emit canceled/cancel-rejected events.
        anyhow::bail!("BingX cancel_order not yet implemented")
    }
}
