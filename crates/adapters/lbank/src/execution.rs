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

//! LBank spot execution client (implements [`ExecutionClient`]).
//!
//! Provides spot LIMIT order submission/cancellation over the signed `/v2/supplement/*` REST
//! endpoints and emits the resulting order events. MARKET orders are REJECTED honestly (LBank's
//! spot market-BUY `price` field is a quote-notional spend, not a base amount — submitting a base
//! quantity would be a silent notional bug). Order status / fill reconciliation over the private
//! WebSocket (`subscribeKey` flow) is a follow-up (the trait's default report generators are used
//! until then).

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
    common::{credential::Credential, parse::lbank_symbol_from_instrument_id},
    config::LbankExecClientConfig,
    http::{client::LbankHttpClient, models::LBankCreateOrderRequest},
};

/// Execution client for LBank spot markets.
#[derive(Debug)]
pub struct LbankExecutionClient {
    core: ExecutionClientCore,
    #[allow(dead_code)]
    config: LbankExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: LbankHttpClient,
    clock: &'static AtomicTime,
}

impl LbankExecutionClient {
    /// Creates a new [`LbankExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the HTTP client cannot be constructed.
    pub fn new(core: ExecutionClientCore, config: LbankExecClientConfig) -> anyhow::Result<Self> {
        let credential = Credential::resolve(config.api_key.clone(), config.api_secret.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "LBank credentials not available; set LBANK_API_KEY and LBANK_API_SECRET or pass them in the config"
                )
            })?;

        let mut http_client = LbankHttpClient::with_optional_credentials(
            Some(credential),
            Some(config.http_timeout_secs),
            config.proxy_url.clone(),
        )
        .map_err(|e| anyhow::anyhow!("failed to create LBank HTTP client: {e}"))?;
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

    /// Builds the LBank `type` value for a LIMIT order: the side (`buy`/`sell`) plus a TIF/post-only
    /// suffix (`_ioc`/`_fok`/`_maker`), per CCXT `create_order`.
    fn limit_order_type(side: OrderSide, tif: TimeInForce, post_only: bool) -> String {
        let base = match side {
            OrderSide::Buy => "buy",
            _ => "sell",
        };
        if post_only {
            return format!("{base}_maker");
        }
        match tif {
            TimeInForce::Ioc => format!("{base}_ioc"),
            TimeInForce::Fok => format!("{base}_fok"),
            _ => base.to_string(),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl ExecutionClient for LbankExecutionClient {
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
        log::info!("Started LBank execution client {}", self.core.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.core.set_stopped();
        self.core.set_disconnected();
        log::info!("Stopped LBank execution client {}", self.core.client_id);
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }

        // Bootstrap the account state from spot balances.
        match self.http_client.request_account().await {
            Ok(account) => {
                let mut balances = Vec::with_capacity(account.balances.len());
                for bal in &account.balances {
                    let asset = bal.asset.to_uppercase();
                    let currency = nautilus_model::types::Currency::get_or_create_crypto(&asset);
                    let free = bal
                        .free
                        .as_ref()
                        .and_then(|v| f64::from_str(v.as_str()).ok())
                        .unwrap_or(0.0);
                    let locked = bal
                        .locked
                        .as_ref()
                        .and_then(|v| f64::from_str(v.as_str()).ok())
                        .unwrap_or(0.0);
                    let total = free + locked;
                    if total == 0.0 {
                        continue;
                    }
                    balances.push(AccountBalance::new(
                        Money::new(total, currency),
                        Money::new(locked, currency),
                        Money::new(free, currency),
                    ));
                }
                self.emitter
                    .emit_account_state(balances, vec![], true, self.clock.get_time_ns());
            }
            Err(e) => log::warn!("Failed to bootstrap LBank account state: {e}"),
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

        // Only LIMIT orders are supported: LBank spot market-BUY `price` is a quote-notional spend
        // (not a base amount), so submitting a base qty would be a silent notional bug — reject.
        if order.order_type() != OrderType::Limit {
            self.emitter.emit_order_rejected(
                &order,
                "LBank spot adapter currently supports LIMIT orders only (market-buy uses quote notional)",
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

        let request = LBankCreateOrderRequest {
            symbol: lbank_symbol_from_instrument_id(&order.instrument_id()),
            order_type: Self::limit_order_type(
                order.order_side(),
                order.time_in_force(),
                order.is_post_only(),
            ),
            price: price.to_string(),
            amount: order.quantity().to_string(),
            custom_id: Some(order.client_order_id().to_string()),
        };

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        get_runtime().spawn(async move {
            match http_client.create_order(&request).await {
                Ok(resp) => match resp.order_id {
                    Some(id) if !id.is_empty() => {
                        emitter.emit_order_accepted(
                            &order,
                            VenueOrderId::new(&id),
                            clock.get_time_ns(),
                        );
                    }
                    _ => emitter.emit_order_rejected(
                        &order,
                        "LBank create_order returned no order_id",
                        clock.get_time_ns(),
                        false,
                    ),
                },
                Err(e) => {
                    emitter.emit_order_rejected(
                        &order,
                        &format!("LBank order rejected: {e}"),
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

        let symbol = lbank_symbol_from_instrument_id(&order.instrument_id());
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        get_runtime().spawn(async move {
            match http_client.cancel_order(&symbol, venue_order_id.as_str()).await {
                Ok(_) => {
                    emitter.emit_order_canceled(&order, Some(venue_order_id), clock.get_time_ns());
                }
                Err(e) => {
                    emitter.emit_order_cancel_rejected(
                        &order,
                        Some(venue_order_id),
                        &format!("LBank cancel failed: {e}"),
                        clock.get_time_ns(),
                    );
                }
            }
        });

        Ok(())
    }
}
