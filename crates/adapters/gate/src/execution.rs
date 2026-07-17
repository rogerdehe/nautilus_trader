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

//! Gate spot execution client (implements [`ExecutionClient`]).
//!
//! Provides spot order submission/cancellation over the signed APIv4 REST endpoints and emits the
//! resulting order events. Order status / fill reconciliation over the private WebSocket channel is
//! a follow-up (the trait's default report generators are used until then).

use std::str::FromStr;

use nautilus_common::{
    clients::ExecutionClient,
    live::runtime::get_runtime,
    messages::execution::{CancelOrder, SubmitOrder},
};
use nautilus_core::{
    UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{OmsType, OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientId, VenueOrderId},
    orders::Order,
    types::{AccountBalance, MarginBalance, Money},
};

use crate::{
    common::{credential::Credential, parse::gate_symbol_from_instrument_id},
    config::GateExecClientConfig,
    http::{client::GateHttpClient, models::SpotOrderRequest},
};

/// Execution client for Gate spot markets.
#[derive(Debug)]
pub struct GateExecutionClient {
    core: ExecutionClientCore,
    #[allow(dead_code)]
    config: GateExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: GateHttpClient,
    clock: &'static AtomicTime,
}

impl GateExecutionClient {
    /// Creates a new [`GateExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the HTTP client cannot be constructed.
    pub fn new(core: ExecutionClientCore, config: GateExecClientConfig) -> anyhow::Result<Self> {
        let credential = Credential::resolve(config.api_key.clone(), config.api_secret.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Gate credentials not available; set GATE_API_KEY and GATE_API_SECRET or pass them in the config"
                )
            })?;

        let mut http_client = GateHttpClient::with_optional_credentials(
            Some(credential),
            Some(config.http_timeout_secs),
            config.proxy_url.clone(),
        )
        .map_err(|e| anyhow::anyhow!("failed to create Gate HTTP client: {e}"))?;
        if let Some(url) = &config.base_url_http {
            http_client.set_base_url(url.clone());
        }

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

    /// Maps a Nautilus [`TimeInForce`] onto Gate's `time_in_force`, honoring post-only.
    fn map_tif(tif: TimeInForce, post_only: bool) -> Option<String> {
        if post_only {
            return Some("poc".to_string());
        }
        match tif {
            TimeInForce::Gtc => Some("gtc".to_string()),
            TimeInForce::Ioc => Some("ioc".to_string()),
            TimeInForce::Fok => Some("fok".to_string()),
            _ => None,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl ExecutionClient for GateExecutionClient {
    fn is_connected(&self) -> bool {
        self.core.is_connected()
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> nautilus_model::identifiers::Venue {
        self.core.venue
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
        self.core.set_started();
        log::info!("Started Gate execution client {}", self.core.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.core.set_stopped();
        self.core.set_disconnected();
        log::info!("Stopped Gate execution client {}", self.core.client_id);
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }

        // Bootstrap the account state from spot balances.
        match self.http_client.request_spot_accounts().await {
            Ok(accounts) => {
                let mut balances = Vec::with_capacity(accounts.len());
                for acct in &accounts {
                    let currency =
                        nautilus_model::types::Currency::get_or_create_crypto(&acct.currency);
                    let available = acct
                        .available
                        .as_deref()
                        .and_then(|v| f64::from_str(v).ok())
                        .unwrap_or(0.0);
                    let locked = acct
                        .locked
                        .as_deref()
                        .and_then(|v| f64::from_str(v).ok())
                        .unwrap_or(0.0);
                    let total = available + locked;
                    balances.push(AccountBalance::new(
                        Money::new(total, currency),
                        Money::new(locked, currency),
                        Money::new(available, currency),
                    ));
                }
                self.emitter
                    .emit_account_state(balances, vec![], true, self.clock.get_time_ns());
            }
            Err(e) => log::warn!("Failed to bootstrap Gate account state: {e}"),
        }

        self.core.set_connected();
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.core.set_disconnected();
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.cache().try_order_owned(&cmd.client_order_id)?;
        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        self.emitter.emit_order_submitted(&order);

        // Only limit orders are supported over the spot REST path for now; market orders require
        // side-specific quote/base amount handling (Gate market-buy amount is quote-denominated).
        if order.order_type() != OrderType::Limit {
            self.emitter.emit_order_rejected(
                &order,
                "Gate spot adapter currently supports LIMIT orders only",
                self.clock.get_time_ns(),
                false,
            );
            return Ok(());
        }
        let Some(price) = order.price() else {
            self.emitter.emit_order_rejected(
                &order,
                "LIMIT order missing price",
                self.clock.get_time_ns(),
                false,
            );
            return Ok(());
        };

        let request = SpotOrderRequest {
            currency_pair: gate_symbol_from_instrument_id(&order.instrument_id()),
            side: match order.order_side() {
                OrderSide::Buy => "buy".to_string(),
                _ => "sell".to_string(),
            },
            order_type: "limit".to_string(),
            amount: order.quantity().to_string(),
            price: Some(price.to_string()),
            time_in_force: Self::map_tif(order.time_in_force(), order.is_post_only()),
            text: None,
        };

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let order = order.clone();
        get_runtime().spawn(async move {
            match http_client.place_spot_order(&request).await {
                Ok(resp) => {
                    emitter.emit_order_accepted(
                        &order,
                        VenueOrderId::new(&resp.id),
                        clock.get_time_ns(),
                    );
                }
                Err(e) => {
                    emitter.emit_order_rejected(
                        &order,
                        &format!("Gate order rejected: {e}"),
                        clock.get_time_ns(),
                        false,
                    );
                }
            }
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let order = match self.core.cache().try_order_owned(&cmd.client_order_id) {
            Ok(order) => order,
            Err(e) => {
                log::warn!("Cannot cancel unknown order {}: {e}", cmd.client_order_id);
                return Ok(());
            }
        };
        let Some(venue_order_id) = cmd.venue_order_id.or_else(|| order.venue_order_id()) else {
            self.emitter.emit_order_cancel_rejected(
                &order,
                None,
                "cancel requires a venue_order_id",
                self.clock.get_time_ns(),
            );
            return Ok(());
        };

        let currency_pair = gate_symbol_from_instrument_id(&order.instrument_id());
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let order = order.clone();
        get_runtime().spawn(async move {
            match http_client
                .cancel_spot_order(venue_order_id.as_str(), &currency_pair)
                .await
            {
                Ok(_) => {
                    emitter.emit_order_canceled(
                        &order,
                        Some(venue_order_id),
                        clock.get_time_ns(),
                    );
                }
                Err(e) => {
                    emitter.emit_order_cancel_rejected(
                        &order,
                        Some(venue_order_id),
                        &format!("Gate cancel failed: {e}"),
                        clock.get_time_ns(),
                    );
                }
            }
        });

        Ok(())
    }
}
