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

//! Live execution client for the KuCoin spot adapter.
//!
//! Thin but functional: [`submit_order`](KuCoinExecutionClient::submit_order) /
//! [`cancel_order`](KuCoinExecutionClient::cancel_order) translate directly to KuCoin's
//! `POST /api/v1/orders` and `DELETE /api/v1/orders/{orderId}` (body shape first-hand from
//! `ccxt/python/ccxt/kucoin.py::create_order`); [`connect`](KuCoinExecutionClient::connect) loads
//! and emits the account state. Order/fill status reconciliation is available via the HTTP client's
//! report parsers but is not yet auto-scheduled (see crate docs / report gaps).

use nautilus_common::{
    clients::ExecutionClient,
    live::{runner::get_exec_event_sender, runtime::get_runtime},
    messages::execution::{CancelOrder, SubmitOrder},
};
use nautilus_core::time::{AtomicTime, get_atomic_clock_realtime};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{OmsType, OrderType},
    identifiers::{AccountId, ClientId, Venue},
};
use serde_json::json;

use crate::{
    common::consts::kucoin_venue,
    config::KuCoinExecClientConfig,
    http::client::KuCoinHttpClient,
};

/// Live execution client for KuCoin spot.
pub struct KuCoinExecutionClient {
    core: ExecutionClientCore,
    #[allow(dead_code)] // Retained for parity/future signed WS order stream.
    config: KuCoinExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: KuCoinHttpClient,
}

impl std::fmt::Debug for KuCoinExecutionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KuCoinExecutionClient))
            .field("client_id", &self.core.client_id)
            .field("account_id", &self.core.account_id)
            .finish()
    }
}

impl KuCoinExecutionClient {
    /// Creates a new [`KuCoinExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the HTTP client cannot be constructed.
    pub fn new(
        core: ExecutionClientCore,
        config: KuCoinExecClientConfig,
    ) -> anyhow::Result<Self> {
        let clock: &'static AtomicTime = get_atomic_clock_realtime();
        let (key, secret, passphrase) = resolve_creds(&config)
            .ok_or_else(|| anyhow::anyhow!("KuCoin execution client requires API credentials"))?;
        let http_client =
            KuCoinHttpClient::with_credentials(key, secret, passphrase, config.base_url_http.clone())
                .map_err(|e| anyhow::anyhow!("Failed to build KuCoin HTTP client: {e}"))?;

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
        })
    }
}

fn resolve_creds(config: &KuCoinExecClientConfig) -> Option<(String, String, String)> {
    use nautilus_core::env::get_or_env_var_opt;
    use crate::common::credential::credential_env_vars;
    let (k, s, p) = credential_env_vars();
    Some((
        get_or_env_var_opt(config.api_key.clone(), k)?,
        get_or_env_var_opt(config.api_secret.clone(), s)?,
        get_or_env_var_opt(config.api_passphrase.clone(), p)?,
    ))
}

#[async_trait::async_trait(?Send)]
impl ExecutionClient for KuCoinExecutionClient {
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
        kucoin_venue()
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account_owned(&self.core.account_id)
    }

    fn generate_account_state(
        &self,
        balances: Vec<nautilus_model::types::AccountBalance>,
        margins: Vec<nautilus_model::types::MarginBalance>,
        reported: bool,
        ts_event: nautilus_core::UnixNanos,
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
        // Prime the instrument cache (precisions) for report parsing.
        if let Err(e) = self.http_client.request_instruments().await {
            log::warn!("Failed to load KuCoin instruments for execution: {e}");
        }
        let account_state = self
            .http_client
            .request_account_state(self.core.account_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to request KuCoin account state: {e}"))?;
        self.emitter.send_account_state(account_state);
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

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let init = &cmd.order_init;
        let symbol = cmd.instrument_id.symbol.as_str().to_string();
        let side = match init.order_side {
            nautilus_model::enums::OrderSide::Buy => "buy",
            nautilus_model::enums::OrderSide::Sell => "sell",
            nautilus_model::enums::OrderSide::NoOrderSide => {
                anyhow::bail!("Cannot submit order with no side")
            }
        };
        let client_oid = cmd.client_order_id.as_str().to_string();
        let size = init.quantity.to_string();

        let body = match init.order_type {
            OrderType::Limit => {
                let price = init
                    .price
                    .ok_or_else(|| anyhow::anyhow!("Limit order requires a price"))?;
                let tif = match init.time_in_force {
                    nautilus_model::enums::TimeInForce::Ioc => "IOC",
                    nautilus_model::enums::TimeInForce::Fok => "FOK",
                    _ => "GTC",
                };
                json!({
                    "clientOid": client_oid,
                    "symbol": symbol,
                    "side": side,
                    "type": "limit",
                    "price": price.to_string(),
                    "size": size,
                    "timeInForce": tif,
                    "postOnly": init.post_only,
                })
            }
            OrderType::Market => json!({
                "clientOid": client_oid,
                "symbol": symbol,
                "side": side,
                "type": "market",
                "size": size,
            }),
            other => anyhow::bail!("KuCoin spot adapter does not support order type {other:?}"),
        };

        let http = self.http_client.clone();
        let body_str = body.to_string();
        let client_order_id = cmd.client_order_id;
        get_runtime().spawn(async move {
            match http.submit_order(body_str).await {
                Ok(order_id) => {
                    log::info!("Submitted KuCoin order {client_order_id} -> venue {order_id}");
                }
                Err(e) => log::error!("Failed to submit KuCoin order {client_order_id}: {e}"),
            }
        });
        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let Some(venue_order_id) = cmd.venue_order_id else {
            anyhow::bail!("KuCoin cancel requires a venue order id");
        };
        let http = self.http_client.clone();
        let order_id = venue_order_id.as_str().to_string();
        get_runtime().spawn(async move {
            if let Err(e) = http.cancel_order(&order_id).await {
                log::error!("Failed to cancel KuCoin order {order_id}: {e}");
            }
        });
        Ok(())
    }
}
