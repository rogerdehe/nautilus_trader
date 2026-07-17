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

//! Live execution client for the MEXC spot adapter.
//!
//! Order submission/cancellation uses the MEXC v3 REST API. Because the spot WebSocket is
//! protobuf-encoded (documented gap — see [`crate::websocket`]) there is **no live fill stream**;
//! order/fill reconciliation is therefore REST-poll based via `generate_order_status_report(s)` and
//! `generate_fill_reports`.

use nautilus_core::{UnixNanos, time::{AtomicTime, get_atomic_clock_realtime}};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_common::{
    clients::ExecutionClient,
    live::{runner::get_exec_event_sender, runtime::get_runtime},
    messages::execution::{
        CancelOrder, GenerateFillReports, GenerateOrderStatusReport, GenerateOrderStatusReports,
        SubmitOrder,
    },
};
use nautilus_model::{
    accounts::AccountAny,
    enums::{OmsType, OrderSide, OrderType},
    identifiers::{AccountId, ClientId, Venue, VenueOrderId},
    instruments::InstrumentAny,
    orders::Order,
    reports::{FillReport, OrderStatusReport},
    types::{AccountBalance, MarginBalance},
};

use crate::{
    common::{
        consts::MEXC_VENUE,
        enums::{resolve_mexc_order_type, MexcOrderSide},
    },
    config::MexcExecClientConfig,
    http::client::MexcHttpClient,
};

/// A live execution client for MEXC spot.
#[derive(Debug)]
pub struct MexcExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    /// Retained for reconnection / future WS listen-key use.
    #[allow(dead_code)]
    config: MexcExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: MexcHttpClient,
}

impl MexcExecutionClient {
    /// Creates a new [`MexcExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(core: ExecutionClientCore, config: MexcExecClientConfig) -> anyhow::Result<Self> {
        let http_client = MexcHttpClient::with_credentials(
            config.api_key.clone().unwrap_or_default(),
            config.api_secret.clone().unwrap_or_default(),
            Some(config.http_base_url()),
            Some(config.http_timeout_secs),
        )
        .map_err(|e| anyhow::anyhow!("Failed to create MEXC HTTP client: {e}"))?;

        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            core.account_type,
            core.base_currency,
        );

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
        })
    }
}

#[async_trait::async_trait(?Send)]
impl ExecutionClient for MexcExecutionClient {
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
        *MEXC_VENUE
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
        log::info!(
            "Started MEXC execution client: client_id={}, account_id={}",
            self.core.client_id,
            self.core.account_id,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }
        self.core.set_stopped();
        self.core.set_disconnected();
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }
        // Emit an initial account state snapshot from REST balances.
        match self.http_client.request_account_balances().await {
            Ok(balances) => {
                self.emitter
                    .emit_account_state(balances, Vec::new(), true, self.clock.get_time_ns());
            }
            Err(e) => log::warn!("MEXC: failed to fetch account balances on connect: {e}"),
        }
        self.core.set_connected();
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.core.set_disconnected();
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.cache().try_order_owned(&cmd.client_order_id)?;
        if order.is_closed() {
            log::warn!("MEXC: cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        let side = match MexcOrderSide::try_from(order.order_side()) {
            Ok(s) => s,
            Err(e) => {
                self.emitter.emit_order_denied(&order, &format!("{e}"));
                return Ok(());
            }
        };
        let mexc_type = resolve_mexc_order_type(
            order.order_type(),
            order.time_in_force(),
            order.is_post_only(),
        );

        let mut params: Vec<(&str, String)> = vec![
            ("symbol", order.instrument_id().symbol.as_str().to_string()),
            ("side", side.as_str().to_string()),
            ("type", mexc_type.as_str().to_string()),
            ("quantity", order.quantity().to_string()),
            (
                "newClientOrderId",
                order.client_order_id().as_str().to_string(),
            ),
        ];
        if order.order_type() != OrderType::Market
            && let Some(price) = order.price()
        {
            params.push(("price", price.to_string()));
        }

        self.emitter.emit_order_submitted(&order);

        let http = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.submit_order(&params).await {
                Ok(ack) => {
                    let venue_order_id = VenueOrderId::new(ack.order_id.as_str());
                    emitter.emit_order_accepted(&order, venue_order_id, clock.get_time_ns());
                }
                Err(e) => {
                    emitter.emit_order_rejected(
                        &order,
                        &format!("MEXC submit rejected: {e}"),
                        clock.get_time_ns(),
                        false,
                    );
                }
            }
        });
        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let Ok(order) = self.core.cache().try_order_owned(&cmd.client_order_id) else {
            log::warn!("MEXC: cannot cancel unknown order {}", cmd.client_order_id);
            return Ok(());
        };

        let mut params: Vec<(&str, String)> =
            vec![("symbol", cmd.instrument_id.symbol.as_str().to_string())];
        if let Some(vid) = cmd.venue_order_id {
            params.push(("orderId", vid.as_str().to_string()));
        } else {
            params.push((
                "origClientOrderId",
                cmd.client_order_id.as_str().to_string(),
            ));
        }

        let http = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let venue_order_id = cmd.venue_order_id;

        get_runtime().spawn(async move {
            match http.cancel_order(&params).await {
                Ok(_) => {
                    emitter.emit_order_canceled(&order, venue_order_id, clock.get_time_ns());
                }
                Err(e) => log::error!("MEXC: cancel failed for {}: {e}", order.client_order_id()),
            }
        });
        Ok(())
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let reports = self
            .http_client
            .request_order_status_reports(self.core.account_id, cmd.instrument_id, self.clock.get_time_ns())
            .await
            .map_err(|e| anyhow::anyhow!("MEXC order status reports failed: {e}"))?;
        Ok(reports)
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let reports = self
            .http_client
            .request_order_status_reports(self.core.account_id, cmd.instrument_id, self.clock.get_time_ns())
            .await
            .map_err(|e| anyhow::anyhow!("MEXC order status report failed: {e}"))?;

        Ok(reports.into_iter().find(|r| {
            cmd.venue_order_id
                .is_some_and(|vid| r.venue_order_id == vid)
                || cmd
                    .client_order_id
                    .is_some_and(|coid| r.client_order_id == Some(coid))
        }))
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let Some(instrument_id) = cmd.instrument_id else {
            log::warn!("MEXC: generate_fill_reports requires an instrument_id (myTrades needs a symbol)");
            return Ok(Vec::new());
        };
        let reports = self
            .http_client
            .request_fill_reports(self.core.account_id, instrument_id, self.clock.get_time_ns())
            .await
            .map_err(|e| anyhow::anyhow!("MEXC fill reports failed: {e}"))?;
        Ok(reports)
    }

    fn on_instrument(&mut self, _instrument: InstrumentAny) {}
}

#[allow(dead_code)]
fn _side_str(side: OrderSide) -> Option<&'static str> {
    MexcOrderSide::try_from(side).ok().map(MexcOrderSide::as_str)
}
