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

//! LBank CONTRACT (USDT-perp) market data client (implements [`DataClient`]).
//!
//! Bootstraps perpetual instruments over the contract REST (`cfd/openApi/v1/pub/instrument`),
//! subscribes to the v3 OrderBook (top-25 snapshot at native tick) + Deal channels, and forwards
//! parsed data. Depth frames carry no symbol, so each frame is attributed to an instrument via the
//! subscription id (`y`) mapped at subscribe time. Keepalive is an ACTIVE client ping (handled by
//! the WS client's background task).

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Context;
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent,
        data::{
            DataResponse, InstrumentsResponse, RequestInstruments, SubscribeBookDeltas,
            SubscribeInstrument, SubscribeTrades, UnsubscribeBookDeltas, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    AtomicMap,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::{
    common::{
        consts::lbank_venue, credential::Credential, parse::contract_symbol_from_instrument_id,
    },
    config::LbankDataClientConfig,
    http::client::LbankHttpClient,
    websocket::{
        contract_client::LbankContractWebSocketClient,
        contract_messages::{ContractDealData, ContractWsFrame},
        contract_parse::{parse_contract_depth, parse_contract_trade},
    },
};

/// Data client for LBank USDT-perp contract markets.
#[derive(Debug)]
pub struct LbankContractDataClient {
    client_id: ClientId,
    #[allow(dead_code)]
    config: LbankDataClientConfig,
    http_client: LbankHttpClient,
    ws_client: LbankContractWebSocketClient,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    clock: &'static AtomicTime,
}

impl LbankContractDataClient {
    /// Creates a new [`LbankContractDataClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(client_id: ClientId, config: LbankDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        let credential = Credential::resolve(config.api_key.clone(), config.api_secret.clone());
        let mut http_client = LbankHttpClient::with_optional_credentials(
            credential,
            Some(config.http_timeout_secs),
            config.proxy_url.clone(),
        )
        .map_err(|e| anyhow::anyhow!("failed to create LBank HTTP client: {e}"))?;
        // Contract REST lives on a different host from spot.
        http_client.set_base_url(config.contract_http_url());

        let ws_client = LbankContractWebSocketClient::new(&config.contract_ws_url());

        Ok(Self {
            client_id,
            config,
            http_client,
            ws_client,
            instruments: Arc::new(AtomicMap::new()),
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            clock,
        })
    }

    async fn bootstrap_instruments(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        let instruments = self
            .http_client
            .request_contract_instruments()
            .await
            .map_err(|e| anyhow::anyhow!("failed to fetch contract instruments: {e}"))?;

        self.instruments.rcu(|m| {
            for instrument in &instruments {
                m.insert(instrument.id(), instrument.clone());
            }
        });
        self.ws_client.initialize_instruments(instruments.clone());
        log::debug!("Bootstrapped {} LBank contract instruments", instruments.len());
        Ok(instruments)
    }

    async fn spawn_ws(&mut self) -> anyhow::Result<()> {
        self.ws_client
            .connect()
            .await
            .context("failed to connect to LBank contract WebSocket")?;

        let mut raw_rx = self
            .ws_client
            .take_receiver()
            .ok_or_else(|| anyhow::anyhow!("LBank contract WS receiver unavailable"))?;
        let data_sender = self.data_sender.clone();
        let cancellation_token = self.cancellation_token.clone();
        let instruments = Arc::clone(&self.instruments);
        let clock = self.clock;

        let task = get_runtime().spawn(async move {
            log::debug!("LBank contract WebSocket consumption loop started");
            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => break,
                    msg_opt = raw_rx.recv() => {
                        // Each message is tagged with its instrument by the owning per-coin socket
                        // (one socket == one instrument), so attribution needs no subscription-id map.
                        let Some((instrument_id, msg)) = msg_opt else { break };
                        let text = match msg {
                            Message::Text(t) => t.to_string(),
                            Message::Binary(b) => match String::from_utf8(b.to_vec()) {
                                Ok(t) => t,
                                Err(_) => continue,
                            },
                            Message::Close(_) => continue,
                            _ => continue,
                        };
                        forward_contract_text(&text, instrument_id, &instruments, &data_sender, clock);
                    }
                }
            }
            log::debug!("LBank contract WebSocket consumption loop finished");
        });

        self.tasks.push(task);
        Ok(())
    }
}

/// Resolves `(price_precision, size_precision)` for an instrument, if cached.
fn precisions(
    instruments: &AtomicMap<InstrumentId, InstrumentAny>,
    instrument_id: &InstrumentId,
) -> Option<(u8, u8)> {
    instruments
        .get_cloned(instrument_id)
        .map(|i| (i.price_precision(), i.size_precision()))
}

fn forward_contract_text(
    text: &str,
    instrument_id: InstrumentId,
    instruments: &AtomicMap<InstrumentId, InstrumentAny>,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    clock: &'static AtomicTime,
) {
    let frame: ContractWsFrame = match serde_json::from_str(text) {
        Ok(f) => f,
        Err(e) => {
            log::debug!("LBank contract WS parse error: {e}");
            return;
        }
    };

    let ts_init = clock.get_time_ns();

    if frame.is_depth() {
        let Some((price_precision, size_precision)) = precisions(instruments, &instrument_id) else {
            log::debug!("LBank contract depth for uncached instrument {instrument_id}");
            return;
        };
        match parse_contract_depth(&frame, instrument_id, price_precision, size_precision, ts_init) {
            Ok(deltas) => {
                if let Err(e) = data_sender
                    .send(DataEvent::Data(Data::Deltas(OrderBookDeltas_API::new(deltas))))
                {
                    log::error!("Failed to send contract deltas: {e}");
                }
            }
            Err(e) => log::debug!("LBank contract depth parse failed: {e}"),
        }
    } else if frame.is_trade() {
        let Some((price_precision, size_precision)) = precisions(instruments, &instrument_id) else {
            log::debug!("LBank contract trade for uncached instrument {instrument_id}");
            return;
        };
        let trades = match frame.d.as_ref() {
            Some(ContractDealData::One(t)) => std::slice::from_ref(t),
            Some(ContractDealData::Many(v)) => v.as_slice(),
            None => &[],
        };
        for trade in trades {
            match parse_contract_trade(trade, instrument_id, price_precision, size_precision, ts_init)
            {
                Ok(tick) => {
                    if let Err(e) = data_sender.send(DataEvent::Data(Data::Trade(tick))) {
                        log::error!("Failed to send contract trade: {e}");
                    }
                }
                Err(e) => log::debug!("LBank contract trade parse failed: {e}"),
            }
        }
    }
    // else: heartbeat/ack — ignore.
}

#[async_trait::async_trait(?Send)]
impl DataClient for LbankContractDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(lbank_venue())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!("Starting LBank contract data client {}", self.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping LBank contract data client {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.stop()
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Acquire)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }
        self.cancellation_token = CancellationToken::new();

        let instruments = self
            .bootstrap_instruments()
            .await
            .context("failed to bootstrap contract instruments")?;
        for instrument in instruments {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        self.spawn_ws()
            .await
            .context("failed to spawn LBank contract WebSocket")?;
        self.is_connected.store(true, Ordering::Relaxed);
        log::info!("Connected: client_id={}", self.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Ok(());
        }
        self.cancellation_token.cancel();
        for task in self.tasks.drain(..) {
            if let Err(e) = task.await {
                log::error!("Error awaiting task: {e}");
            }
        }
        self.ws_client.disconnect().await;
        self.instruments.store(ahash::AHashMap::new());
        self.is_connected.store(false, Ordering::Relaxed);
        log::info!("Disconnected: client_id={}", self.client_id);
        Ok(())
    }

    fn subscribe_instrument(&mut self, cmd: SubscribeInstrument) -> anyhow::Result<()> {
        let instruments = self.instruments.load();
        if let Some(instrument) = instruments.get(&cmd.instrument_id) {
            if let Err(e) = self
                .data_sender
                .send(DataEvent::Instrument(instrument.clone()))
            {
                log::error!("Failed to send instrument {}: {e}", cmd.instrument_id);
            }
        } else {
            log::warn!("Instrument {} not in cache", cmd.instrument_id);
        }
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("LBank contract only supports L2_MBP order book deltas");
        }
        let instrument_id = cmd.instrument_id;
        let (symbol, price_tick) = {
            let instruments = self.instruments.load();
            let instrument = instruments
                .get(&instrument_id)
                .with_context(|| format!("instrument {instrument_id} not cached"))?;
            (
                contract_symbol_from_instrument_id(&instrument_id),
                instrument.price_increment().to_string(),
            )
        };
        let ws = self.ws_client.clone();
        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_book(instrument_id, &symbol, &price_tick).await {
                log::error!("Failed to subscribe contract book {instrument_id}: {e:?}");
            }
        });
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let symbol = contract_symbol_from_instrument_id(&instrument_id);
        let ws = self.ws_client.clone();
        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_trades(instrument_id, &symbol).await {
                log::error!("Failed to subscribe contract trades {instrument_id}: {e:?}");
            }
        });
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, _cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_trades(&mut self, _cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = lbank_venue();
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http_client.request_contract_instruments().await {
                Ok(instruments) => {
                    instruments_cache.rcu(|m| {
                        for instrument in &instruments {
                            m.insert(instrument.id(), instrument.clone());
                        }
                    });
                    let response = DataResponse::Instruments(InstrumentsResponse::new(
                        request_id,
                        client_id,
                        venue,
                        instruments,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));
                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send instruments response: {e}");
                    }
                }
                Err(e) => log::error!("Failed to fetch contract instruments: {e:?}"),
            }
        });
        Ok(())
    }
}
