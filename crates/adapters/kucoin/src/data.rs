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

//! Live market-data client for the KuCoin spot adapter.
//!
//! Data flow: the [`KuCoinWebSocketClient`] parses pushes into [`Data`] and pushes them onto an
//! internal channel; [`KuCoinDataClient::connect`] spawns a drain task forwarding each [`Data`]
//! to the engine's data-event sender (`get_data_event_sender`). Instruments are fetched and their
//! precisions registered with the WS client BEFORE any subscribe, so pushes parse at the correct
//! price/size precision.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent,
        data::{
            DataResponse, InstrumentsResponse, RequestInstruments, RequestTrades,
            SubscribeBookDeltas, SubscribeTrades, TradesResponse, UnsubscribeBookDeltas,
            UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::Data,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;

use crate::{
    common::consts::kucoin_venue,
    config::KuCoinDataClientConfig,
    http::client::KuCoinHttpClient,
    websocket::client::KuCoinWebSocketClient,
};

/// Live market-data client for KuCoin spot (order book deltas + trades).
pub struct KuCoinDataClient {
    client_id: ClientId,
    #[allow(dead_code)] // Retained for parity/future signed data channels.
    config: KuCoinDataClientConfig,
    http_client: KuCoinHttpClient,
    ws_client: KuCoinWebSocketClient,
    ws_data_rx: Option<UnboundedReceiver<Data>>,
    is_connected: AtomicBool,
    data_sender: UnboundedSender<DataEvent>,
    instruments: Arc<Mutex<AHashMap<InstrumentId, InstrumentAny>>>,
    cancel: CancellationToken,
    clock: &'static AtomicTime,
}

impl std::fmt::Debug for KuCoinDataClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KuCoinDataClient))
            .field("client_id", &self.client_id)
            .field("is_connected", &self.is_connected.load(Ordering::Relaxed))
            .finish()
    }
}

impl KuCoinDataClient {
    /// Creates a new [`KuCoinDataClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(client_id: ClientId, config: KuCoinDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();
        let http_client = KuCoinHttpClient::new(config.base_url_http.clone())
            .map_err(|e| anyhow::anyhow!("Failed to build KuCoin HTTP client: {e}"))?;

        let (ws_tx, ws_rx) = tokio::sync::mpsc::unbounded_channel::<Data>();
        let ws_client = KuCoinWebSocketClient::new(http_client.clone(), ws_tx);

        Ok(Self {
            client_id,
            config,
            http_client,
            ws_client,
            ws_data_rx: Some(ws_rx),
            is_connected: AtomicBool::new(false),
            data_sender,
            instruments: Arc::new(Mutex::new(AHashMap::new())),
            cancel: CancellationToken::new(),
            clock,
        })
    }

    fn spawn_ws<F>(fut: F, context: &'static str)
    where
        F: std::future::Future<Output = Result<(), crate::http::error::KuCoinHttpError>>
            + Send
            + 'static,
    {
        get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                log::error!("KuCoin {context} failed: {e}");
            }
        });
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for KuCoinDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(kucoin_venue())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.cancel.cancel();
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.cancel.cancel();
        self.ws_client.close();
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        // 1. Fetch instruments and register precisions BEFORE any subscribe.
        let instruments = self
            .http_client
            .request_instruments()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to request KuCoin instruments: {e}"))?;
        {
            let mut guard = self.instruments.lock().expect("instruments lock poisoned");
            for inst in &instruments {
                guard.insert(inst.id(), inst.clone());
            }
        }
        self.ws_client.set_instruments(&instruments);
        log::info!("Loaded {} KuCoin instruments", instruments.len());

        // 2. Spawn the drain: WS `Data` -> engine data-event sender.
        if let Some(mut rx) = self.ws_data_rx.take() {
            let sender = self.data_sender.clone();
            let cancel = self.cancel.clone();
            get_runtime().spawn(async move {
                loop {
                    tokio::select! {
                        () = cancel.cancelled() => break,
                        maybe = rx.recv() => match maybe {
                            Some(data) => {
                                if let Err(e) = sender.send(DataEvent::Data(data)) {
                                    log::error!("Failed to forward KuCoin data: {e}");
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                }
                log::debug!("KuCoin data drain task stopped");
            });
        }

        // 3. Connect the WebSocket (negotiates a bullet token internally).
        self.ws_client
            .connect()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect KuCoin websocket: {e}"))?;

        self.is_connected.store(true, Ordering::Relaxed);
        log::info!("Connected: client_id={}", self.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.is_disconnected() {
            return Ok(());
        }
        self.cancel.cancel();
        self.ws_client.close();
        self.is_connected.store(false, Ordering::Relaxed);
        log::info!("Disconnected: client_id={}", self.client_id);
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;
        Self::spawn_ws(
            async move { ws.subscribe_book(instrument_id).await },
            "book deltas subscription",
        );
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;
        Self::spawn_ws(
            async move { ws.subscribe_trades(instrument_id).await },
            "trades subscription",
        );
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, _cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        // KuCoin supports unsubscribe frames; not yet wired (best-effort noop).
        Ok(())
    }

    fn unsubscribe_trades(&mut self, _cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = kucoin_venue();
        let start = request.start;
        let end = request.end;
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            let instruments = match http.request_instruments().await {
                Ok(i) => i,
                Err(e) => {
                    log::error!("Failed to fetch KuCoin instruments: {e}");
                    Vec::new()
                }
            };
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
                log::error!("Failed to send KuCoin instruments response: {e}");
            }
        });
        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let instrument_id = request.instrument_id;
        let start = request.start;
        let end = request.end;
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            let trades = match http.request_trades(instrument_id).await {
                Ok(t) => t,
                Err(e) => {
                    log::error!("Failed to fetch KuCoin trades for {instrument_id}: {e}");
                    Vec::new()
                }
            };
            let response = DataResponse::Trades(TradesResponse::new(
                request_id,
                client_id,
                instrument_id,
                trades,
                start_nanos,
                end_nanos,
                clock.get_time_ns(),
                params,
            ));
            if let Err(e) = sender.send(DataEvent::Response(response)) {
                log::error!("Failed to send KuCoin trades response: {e}");
            }
        });
        Ok(())
    }
}
