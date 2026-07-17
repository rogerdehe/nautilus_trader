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

//! WebSocket client for the KuCoin spot public data path.
//!
//! Connection handshake (first-hand from `ccxt/python/ccxt/pro/kucoin.py`):
//! 1. `POST /api/v1/bullet-public` for a token + `instanceServers[0].{endpoint,pingInterval}`.
//! 2. Connect to `endpoint?token=<token>&connectId=public`.
//! 3. Subscribe with `{"id","type":"subscribe","topic":"/spotMarket/level2Depth50:BTC-USDT",...}`.
//! 4. Keep alive with a JSON `{"id","type":"ping"}` every `pingInterval` ms.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use ahash::AHashMap;
use futures_util::StreamExt;
use nautilus_model::{
    data::Data,
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    Message,
    websocket::{WebSocketClient, WebSocketConfig, types::MessageReader},
};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use super::{
    messages::{KuCoinWsDepth, KuCoinWsMessage, KuCoinWsPing, KuCoinWsSubscribe, KuCoinWsTrade},
    parse::{parse_depth_snapshot, parse_ws_trade},
};
use crate::{
    common::parse::instrument_id_from_kucoin_symbol,
    http::{client::KuCoinHttpClient, error::KuCoinHttpError},
};

/// WS topic prefix for the full top-50 spot order book snapshot channel.
const TOPIC_DEPTH50: &str = "/spotMarket/level2Depth50:";
/// WS topic prefix for the spot public trade (match) channel.
const TOPIC_MATCH: &str = "/market/match:";

type PrecisionMap = Arc<Mutex<AHashMap<InstrumentId, (u8, u8)>>>;

/// WebSocket client for KuCoin spot public market data (order book + trades).
#[derive(Clone)]
pub struct KuCoinWebSocketClient {
    http: KuCoinHttpClient,
    client: Arc<Mutex<Option<Arc<WebSocketClient>>>>,
    precisions: PrecisionMap,
    data_sender: UnboundedSender<Data>,
    request_id: Arc<AtomicU64>,
    cancel: CancellationToken,
}

impl std::fmt::Debug for KuCoinWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KuCoinWebSocketClient))
            .field("connected", &self.is_active())
            .finish()
    }
}

impl KuCoinWebSocketClient {
    /// Creates a new [`KuCoinWebSocketClient`].
    ///
    /// `data_sender` receives parsed [`Data`] (deltas + trades) for forwarding to the engine.
    #[must_use]
    pub fn new(http: KuCoinHttpClient, data_sender: UnboundedSender<Data>) -> Self {
        Self {
            http,
            client: Arc::new(Mutex::new(None)),
            precisions: Arc::new(Mutex::new(AHashMap::new())),
            data_sender,
            request_id: Arc::new(AtomicU64::new(1)),
            cancel: CancellationToken::new(),
        }
    }

    /// Registers instrument precisions so pushes can be parsed to correctly-scaled prices/sizes.
    pub fn set_instruments(&self, instruments: &[InstrumentAny]) {
        let mut guard = self.precisions.lock().expect("precisions lock poisoned");
        for inst in instruments {
            guard.insert(inst.id(), (inst.price_precision(), inst.size_precision()));
        }
    }

    /// Returns `true` if the underlying transport is connected/active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.client
            .lock()
            .expect("client lock poisoned")
            .as_ref()
            .is_some_and(|c| c.is_active())
    }

    fn next_id(&self) -> String {
        self.request_id.fetch_add(1, Ordering::Relaxed).to_string()
    }

    /// Connects to KuCoin's public spot WebSocket after negotiating a bullet token.
    ///
    /// # Errors
    ///
    /// Returns an error if the bullet token request fails or the WebSocket cannot connect.
    pub async fn connect(&self) -> Result<(), KuCoinHttpError> {
        let bullet = self.http.request_bullet_public().await?;
        let server = bullet
            .instance_servers
            .first()
            .ok_or_else(|| KuCoinHttpError::ValidationError("No instanceServers".to_string()))?;
        let ping_interval_ms = server.ping_interval;
        let url = format!(
            "{}?token={}&connectId=public",
            server.endpoint, bullet.token
        );

        let config = WebSocketConfig {
            url,
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_delay_max_ms: None,
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: Default::default(),
            proxy_url: None,
        };

        let (reader, client) = WebSocketClient::connect_stream(config, vec![], None, None)
            .await
            .map_err(|e| KuCoinHttpError::ValidationError(format!("WS connect failed: {e}")))?;
        let client = Arc::new(client);
        *self.client.lock().expect("client lock poisoned") = Some(client.clone());

        self.spawn_read_task(reader);
        self.spawn_ping_task(client, ping_interval_ms.max(10_000));
        Ok(())
    }

    /// Subscribes to the full top-50 order book snapshot stream for `instrument_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or the send fails.
    pub async fn subscribe_book(&self, instrument_id: InstrumentId) -> Result<(), KuCoinHttpError> {
        let topic = format!("{TOPIC_DEPTH50}{}", instrument_id.symbol.as_str());
        self.send_subscribe(topic).await
    }

    /// Subscribes to the public trade (match) stream for `instrument_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or the send fails.
    pub async fn subscribe_trades(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KuCoinHttpError> {
        let topic = format!("{TOPIC_MATCH}{}", instrument_id.symbol.as_str());
        self.send_subscribe(topic).await
    }

    async fn send_subscribe(&self, topic: String) -> Result<(), KuCoinHttpError> {
        let sub = KuCoinWsSubscribe::subscribe(self.next_id(), topic);
        let text = serde_json::to_string(&sub)?;
        let client = self
            .client
            .lock()
            .expect("client lock poisoned")
            .clone()
            .ok_or_else(|| KuCoinHttpError::ValidationError("WS not connected".to_string()))?;
        client
            .send_text(text, None)
            .await
            .map_err(|e| KuCoinHttpError::ValidationError(format!("WS send failed: {e}")))
    }

    /// Closes the WebSocket connection and stops background tasks.
    pub fn close(&self) {
        self.cancel.cancel();
    }

    fn spawn_read_task(&self, mut reader: MessageReader) {
        let precisions = self.precisions.clone();
        let sender = self.data_sender.clone();
        let cancel = self.cancel.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = cancel.cancelled() => break,
                    next = reader.next() => {
                        match next {
                            Some(Ok(msg)) => {
                                if let Some(text) = msg_as_text(&msg) {
                                    handle_text(&text, &precisions, &sender);
                                }
                            }
                            Some(Err(e)) => {
                                log::warn!("KuCoin WS read error: {e}");
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }
            log::debug!("KuCoin WS read task stopped");
        });
    }

    fn spawn_ping_task(&self, client: Arc<WebSocketClient>, interval_ms: u64) {
        let cancel = self.cancel.clone();
        let request_id = self.request_id.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_millis(interval_ms));
            ticker.tick().await; // consume immediate first tick
            loop {
                tokio::select! {
                    () = cancel.cancelled() => break,
                    _ = ticker.tick() => {
                        let id = request_id.fetch_add(1, Ordering::Relaxed).to_string();
                        let ping = KuCoinWsPing::new(id);
                        if let Ok(text) = serde_json::to_string(&ping)
                            && let Err(e) = client.send_text(text, None).await
                        {
                            log::warn!("KuCoin WS ping failed: {e}");
                            break;
                        }
                    }
                }
            }
            log::debug!("KuCoin WS ping task stopped");
        });
    }
}

fn msg_as_text(msg: &Message) -> Option<String> {
    match msg {
        Message::Text(b) => std::str::from_utf8(b).ok().map(str::to_string),
        Message::Binary(b) => std::str::from_utf8(b).ok().map(str::to_string),
        _ => None,
    }
}

fn precision_for(precisions: &PrecisionMap, instrument_id: &InstrumentId) -> (u8, u8) {
    precisions
        .lock()
        .expect("precisions lock poisoned")
        .get(instrument_id)
        .copied()
        .unwrap_or((8, 8))
}

fn handle_text(text: &str, precisions: &PrecisionMap, sender: &UnboundedSender<Data>) {
    let msg: KuCoinWsMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            log::trace!("KuCoin WS non-model frame: {e}");
            return;
        }
    };

    match msg.msg_type.as_str() {
        "message" => {}
        "welcome" | "ack" | "pong" => return,
        "error" => {
            log::warn!("KuCoin WS error frame: {text}");
            return;
        }
        _ => return,
    }

    let (Some(topic), Some(data)) = (msg.topic.as_deref(), msg.data.as_ref()) else {
        return;
    };
    let clock = nautilus_core::time::get_atomic_clock_realtime();
    let ts_init = clock.get_time_ns();

    if topic.starts_with(TOPIC_DEPTH50) {
        let Some(symbol) = topic.strip_prefix(TOPIC_DEPTH50) else {
            return;
        };
        let instrument_id = instrument_id_from_kucoin_symbol(symbol);
        let (pp, sp) = precision_for(precisions, &instrument_id);
        match serde_json::from_value::<KuCoinWsDepth>(data.clone()) {
            Ok(depth) => match parse_depth_snapshot(&depth, instrument_id, pp, sp, ts_init) {
                Ok(deltas) => {
                    let _ = sender.send(Data::Deltas(
                        nautilus_model::data::OrderBookDeltas_API::new(deltas),
                    ));
                }
                Err(e) => log::warn!("KuCoin WS depth parse error: {e}"),
            },
            Err(e) => log::warn!("KuCoin WS depth decode error: {e}"),
        }
    } else if topic.starts_with(TOPIC_MATCH) {
        let Some(symbol) = topic.strip_prefix(TOPIC_MATCH) else {
            return;
        };
        let instrument_id = instrument_id_from_kucoin_symbol(symbol);
        let (pp, sp) = precision_for(precisions, &instrument_id);
        match serde_json::from_value::<KuCoinWsTrade>(data.clone()) {
            Ok(trade) => match parse_ws_trade(&trade, instrument_id, pp, sp, ts_init) {
                Ok(tick) => {
                    let _ = sender.send(Data::Trade(tick));
                }
                Err(e) => log::warn!("KuCoin WS trade parse error: {e}"),
            },
            Err(e) => log::warn!("KuCoin WS trade decode error: {e}"),
        }
    }
}
