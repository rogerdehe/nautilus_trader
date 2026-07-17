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

//! Gate spot market data client (implements [`DataClient`]).
//!
//! Bootstraps instruments over REST, subscribes to the `spot.order_book` snapshot + `spot.trades`
//! channels over WebSocket, and forwards parsed data onto the Nautilus data event bus.

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
    common::{consts::gate_venue, credential::Credential},
    config::GateDataClientConfig,
    http::client::GateHttpClient,
    websocket::{
        client::GateWebSocketClient,
        messages::GateWsMessage,
        parse::parse_ws_message,
    },
};

/// Data client for Gate spot markets.
#[derive(Debug)]
pub struct GateDataClient {
    client_id: ClientId,
    #[allow(dead_code)]
    config: GateDataClientConfig,
    http_client: GateHttpClient,
    ws_client: GateWebSocketClient,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    clock: &'static AtomicTime,
}

impl GateDataClient {
    /// Creates a new [`GateDataClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(client_id: ClientId, config: GateDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        let credential = Credential::resolve(config.api_key.clone(), config.api_secret.clone());
        let mut http_client = GateHttpClient::with_optional_credentials(
            credential,
            Some(config.http_timeout_secs),
            config.proxy_url.clone(),
        )
        .map_err(|e| anyhow::anyhow!("failed to create Gate HTTP client: {e}"))?;
        if let Some(url) = &config.base_url_http {
            http_client.set_base_url(url.clone());
        }

        let ws_client = GateWebSocketClient::new(&config.ws_url());

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

    fn gate_venue(&self) -> Venue {
        gate_venue()
    }

    async fn bootstrap_instruments(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        let instruments = self
            .http_client
            .request_instruments()
            .await
            .map_err(|e| anyhow::anyhow!("failed to fetch instruments during bootstrap: {e}"))?;

        self.instruments.rcu(|m| {
            for instrument in &instruments {
                m.insert(instrument.id(), instrument.clone());
            }
        });
        self.ws_client.initialize_instruments(instruments.clone());
        log::debug!("Bootstrapped {} Gate instruments", instruments.len());
        Ok(instruments)
    }

    async fn spawn_ws(&mut self) -> anyhow::Result<()> {
        self.ws_client
            .connect()
            .await
            .context("failed to connect to Gate WebSocket")?;

        let mut raw_rx = self
            .ws_client
            .take_receiver()
            .ok_or_else(|| anyhow::anyhow!("Gate WS receiver unavailable"))?;
        let data_sender = self.data_sender.clone();
        let cancellation_token = self.cancellation_token.clone();
        let instruments = Arc::clone(&self.instruments);

        let task = get_runtime().spawn(async move {
            log::debug!("Gate WebSocket consumption loop started");
            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => break,
                    msg_opt = raw_rx.recv() => {
                        let Some(msg) = msg_opt else { break };
                        let text = match msg {
                            Message::Text(t) => t.to_string(),
                            Message::Binary(b) => match String::from_utf8(b.to_vec()) {
                                Ok(t) => t,
                                Err(_) => continue,
                            },
                            Message::Close(_) => break,
                            _ => continue,
                        };
                        let lookup = |id: &InstrumentId| instruments.get_cloned(id);
                        match parse_ws_message(&text, &lookup) {
                            Ok(parsed) => {
                                for msg in parsed {
                                    forward_ws_message(msg, &data_sender);
                                }
                            }
                            Err(e) => log::debug!("Gate WS parse error: {e}"),
                        }
                    }
                }
            }
            log::debug!("Gate WebSocket consumption loop finished");
        });

        self.tasks.push(task);
        Ok(())
    }
}

fn forward_ws_message(
    msg: GateWsMessage,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
) {
    match msg {
        GateWsMessage::Trade(trade) => {
            if let Err(e) = data_sender.send(DataEvent::Data(Data::Trade(trade))) {
                log::error!("Failed to send trade tick: {e}");
            }
        }
        GateWsMessage::Deltas(deltas) => {
            if let Err(e) = data_sender.send(DataEvent::Data(Data::Deltas(
                OrderBookDeltas_API::new(deltas),
            ))) {
                log::error!("Failed to send order book deltas: {e}");
            }
        }
        GateWsMessage::Reconnected => log::info!("Gate WebSocket reconnected"),
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for GateDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(self.gate_venue())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!("Starting Gate data client {}", self.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Gate data client {}", self.client_id);
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
            .context("failed to bootstrap instruments")?;
        for instrument in instruments {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        self.spawn_ws().await.context("failed to spawn Gate WebSocket")?;
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
            anyhow::bail!("Gate spot only supports L2_MBP order book deltas");
        }
        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;
        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_book(&instrument_id).await {
                log::error!("Failed to subscribe book {instrument_id}: {e:?}");
            }
        });
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;
        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_trades(&instrument_id).await {
                log::error!("Failed to subscribe trades {instrument_id}: {e:?}");
            }
        });
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, _cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        // Unsubscribe frames are not yet wired; the managed book simply stops being consumed.
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
        let venue = self.gate_venue();
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http_client.request_instruments().await {
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
                Err(e) => log::error!("Failed to fetch instruments: {e:?}"),
            }
        });
        Ok(())
    }
}
