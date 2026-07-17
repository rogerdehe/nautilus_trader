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

//! Bitget [`ExecutionClient`] implementation (spot orders over v2 REST).
//!
//! Order submission and cancellation go through the signed REST endpoints
//! (`place-order` / `cancel-order`). Order/fill status is available via
//! [`BitgetHttpClient::get_spot_open_orders`]; the private order/fill WebSocket stream is a
//! documented gap (see crate README / task report).

use std::sync::Arc;

use async_trait::async_trait;
use nautilus_common::{
    clients::ExecutionClient,
    live::runtime::get_runtime,
    messages::execution::{CancelOrder, SubmitOrder},
};
use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_model::{
    accounts::AccountAny,
    enums::{OmsType, OrderType, TimeInForce},
    identifiers::{AccountId, ClientId, Venue, VenueOrderId},
    orders::Order,
    types::{AccountBalance, MarginBalance},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};

use crate::{
    common::{consts::BITGET_VENUE, parse::raw_symbol_from_instrument_id},
    config::BitgetExecClientConfig,
    http::{client::BitgetHttpClient, models::{BitgetCancelOrderRequest, BitgetPlaceOrderRequest}},
};

/// Execution client for Bitget spot trading over v2 REST.
pub struct BitgetExecutionClient {
    core: ExecutionClientCore,
    emitter: ExecutionEventEmitter,
    http_client: Arc<BitgetHttpClient>,
}

impl std::fmt::Debug for BitgetExecutionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BitgetExecutionClient")
            .field("client_id", &self.core.client_id)
            .field("account_id", &self.core.account_id)
            .finish()
    }
}

impl BitgetExecutionClient {
    /// Creates a new [`BitgetExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be constructed.
    pub fn new(core: ExecutionClientCore, config: BitgetExecClientConfig) -> anyhow::Result<Self> {
        let http_client = BitgetHttpClient::with_credentials(
            config.api_key.clone(),
            config.api_secret.clone(),
            config.api_passphrase.clone(),
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
            emitter,
            http_client: Arc::new(http_client),
        })
    }
}

fn tif_to_force(tif: TimeInForce, post_only: bool) -> String {
    if post_only {
        return "post_only".to_string();
    }
    match tif {
        TimeInForce::Ioc => "ioc".to_string(),
        TimeInForce::Fok => "fok".to_string(),
        _ => "gtc".to_string(),
    }
}

#[async_trait(?Send)]
impl ExecutionClient for BitgetExecutionClient {
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
        *BITGET_VENUE
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
        ts_event: nautilus_core::UnixNanos,
    ) -> anyhow::Result<()> {
        self.emitter
            .emit_account_state(balances, margins, reported, ts_event);
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        self.core.set_started();
        log::info!("Started Bitget execution client {}", self.core.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.core.set_stopped();
        log::info!("Stopped Bitget execution client {}", self.core.client_id);
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }
        // Report the initial account state so the engine has balances to work with.
        match self.http_client.get_spot_account_state(self.core.account_id).await {
            Ok(state) => self.emitter.send_account_state(state),
            Err(e) => log::warn!("Bitget: failed to fetch initial account state: {e}"),
        }
        self.core.set_connected();
        log::info!("Connected Bitget execution client {}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.core.set_disconnected();
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.get_order(&cmd.client_order_id)?;
        let order_type = order.order_type();
        if !matches!(order_type, OrderType::Limit | OrderType::Market) {
            anyhow::bail!("Bitget only supports LIMIT and MARKET orders, was {order_type:?}");
        }
        let order_side = order.order_side();
        // GOTCHA (CCXT `parse_order`): for a spot MARKET BUY, Bitget's `size` field is the QUOTE
        // notional (cost), not the base amount — every other case uses base amount. Sending base
        // qty here would submit a silently-wrong notional, so reject it explicitly until
        // quote-denominated market-buy sizing is implemented.
        if order_type == OrderType::Market && order_side == nautilus_model::enums::OrderSide::Buy {
            let ts = get_atomic_clock_realtime().get_time_ns();
            self.emitter.emit_order_rejected(
                &order,
                "Bitget spot MARKET BUY requires quote-denominated size — not yet supported (use LIMIT)",
                ts,
                false,
            );
            return Ok(());
        }
        let (raw_symbol, _is_perp) = raw_symbol_from_instrument_id(&cmd.instrument_id);
        let side = match order_side {
            nautilus_model::enums::OrderSide::Sell => "sell",
            _ => "buy",
        };
        let order_type_str = if order_type == OrderType::Market {
            "market"
        } else {
            "limit"
        };
        let force = tif_to_force(order.time_in_force(), order.is_post_only());
        let price = order.price().map(|p| p.to_string());

        let request = BitgetPlaceOrderRequest {
            symbol: raw_symbol,
            side: side.to_string(),
            order_type: order_type_str.to_string(),
            force,
            price: if order_type == OrderType::Market { None } else { price },
            size: order.quantity().to_string(),
            client_oid: Some(cmd.client_order_id.to_string()),
        };

        let http = self.http_client.clone();
        let emitter = self.emitter.clone();
        let order = order.clone();
        get_runtime().spawn(async move {
            let clock = get_atomic_clock_realtime();
            let ts = clock.get_time_ns();
            emitter.emit_order_submitted(&order);
            match http.place_spot_order(&request).await {
                Ok(resp) => {
                    emitter.emit_order_accepted(&order, VenueOrderId::new(&resp.order_id), ts);
                }
                Err(e) => {
                    emitter.emit_order_rejected(&order, &e.to_string(), ts, false);
                }
            }
        });
        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let order = self.core.get_order(&cmd.client_order_id)?;
        let (raw_symbol, _is_perp) = raw_symbol_from_instrument_id(&cmd.instrument_id);
        let request = BitgetCancelOrderRequest {
            symbol: raw_symbol,
            order_id: cmd.venue_order_id.map(|v| v.to_string()),
            client_oid: Some(cmd.client_order_id.to_string()),
        };
        let venue_order_id = cmd.venue_order_id;
        let http = self.http_client.clone();
        let emitter = self.emitter.clone();
        let order = order.clone();
        get_runtime().spawn(async move {
            let ts = get_atomic_clock_realtime().get_time_ns();
            match http.cancel_spot_order(&request).await {
                Ok(()) => emitter.emit_order_canceled(&order, venue_order_id, ts),
                Err(e) => log::error!("Bitget cancel failed: {e}"),
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::cache::Cache;
    use nautilus_model::{
        enums::{AccountType, OmsType},
        identifiers::{AccountId, TraderId},
    };
    use rstest::rstest;

    use super::*;

    fn make_core() -> ExecutionClientCore {
        let cache = std::rc::Rc::new(std::cell::RefCell::new(Cache::default()));
        let cache_view: nautilus_common::cache::CacheView = cache.into();
        ExecutionClientCore::new(
            TraderId::from("TRADER-001"),
            ClientId::from("BITGET"),
            *BITGET_VENUE,
            OmsType::Hedging,
            AccountId::from("BITGET-001"),
            AccountType::Cash,
            None,
            cache_view,
        )
    }

    #[rstest]
    fn test_exec_client_construction() {
        let config = BitgetExecClientConfig {
            api_key: Some("k".to_string()),
            api_secret: Some("s".to_string()),
            api_passphrase: Some("p".to_string()),
            ..Default::default()
        };
        let client = BitgetExecutionClient::new(make_core(), config).unwrap();
        assert_eq!(client.client_id(), ClientId::from("BITGET"));
        assert_eq!(client.venue(), *BITGET_VENUE);
        assert_eq!(client.oms_type(), OmsType::Hedging);
    }
}
