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

//! Public-stream WebSocket client for HashKey Global.
//!
//! Connects to `wss://stream-glb.hashkey.com/quote/ws/v1`, subscribes to depth/trade topics
//! (`{"symbol", "topic", "event": "sub"}`), and forwards fully-parsed Nautilus data on an internal
//! channel. Each depth push is a full snapshot. HashKey emits application-level `{"ping": ts}`
//! frames which are answered with `{"pong": ts}`.

use std::{sync::Arc, time::Duration};

use dashmap::DashMap;
use futures_util::Stream;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::Data,
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::{
    MessageHandler, PingHandler, TransportBackend, WebSocketClient, WebSocketConfig,
    channel_message_handler,
};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use crate::websocket::{
    messages::{HashKeyWsDepth, HashKeyWsMessage, HashKeyWsPing, HashKeyWsPush, HashKeyWsTrade},
    parse::{parse_ws_depth, parse_ws_trade},
};

/// Cached instrument metadata needed to parse public-stream messages.
type PrecisionMap = Arc<DashMap<Ustr, (InstrumentId, u8, u8)>>;

/// A public-stream WebSocket client for the HashKey Global exchange.
pub struct HashKeyWebSocketClient {
    url: String,
    backend: TransportBackend,
    proxy_url: Option<String>,
    inner: Option<Arc<WebSocketClient>>,
    out_rx: Option<UnboundedReceiver<HashKeyWsMessage>>,
    instruments: PrecisionMap,
}

impl std::fmt::Debug for HashKeyWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(HashKeyWebSocketClient))
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

impl HashKeyWebSocketClient {
    /// Creates a new [`HashKeyWebSocketClient`] for the given public-stream `url`.
    #[must_use]
    pub fn new(url: String, backend: TransportBackend, proxy_url: Option<String>) -> Self {
        Self {
            url,
            backend,
            proxy_url,
            inner: None,
            out_rx: None,
            instruments: Arc::new(DashMap::new()),
        }
    }

    /// Caches instrument precision so the read loop can parse pushes for these symbols.
    pub fn cache_instruments(&self, instruments: &[InstrumentAny]) {
        for instrument in instruments {
            let symbol = Ustr::from(instrument.raw_symbol().as_str());
            self.instruments.insert(
                symbol,
                (
                    instrument.id(),
                    instrument.price_precision(),
                    instrument.size_precision(),
                ),
            );
        }
    }

    /// Returns `true` when the underlying connection is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.inner.as_ref().is_some_and(|c| c.is_active())
    }

    /// Returns a cheap handle to the underlying connected client for sending subscribe frames.
    #[must_use]
    pub fn client(&self) -> Option<Arc<WebSocketClient>> {
        self.inner.clone()
    }

    /// Builds a HashKey public-stream subscribe frame for `symbol` and `topic`.
    #[must_use]
    pub fn subscribe_frame(symbol: &str, topic: &str) -> String {
        format!("{{\"symbol\":\"{symbol}\",\"topic\":\"{topic}\",\"event\":\"sub\"}}")
    }

    /// Connects to the public stream and starts the read loop.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket connection cannot be established.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let (message_handler, mut raw_rx): (MessageHandler, _) = channel_message_handler();
        let ping_handler: PingHandler = Arc::new(|_payload: Vec<u8>| {});

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: None,
            reconnect_delay_max_ms: None,
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: self.backend,
            proxy_url: self.proxy_url.clone(),
        };

        let client = WebSocketClient::connect(
            config,
            Some(message_handler),
            Some(ping_handler),
            None,
            vec![],
            None,
        )
        .await?;
        let client = Arc::new(client);

        let (out_tx, out_rx) = unbounded_channel::<HashKeyWsMessage>();
        let instruments = self.instruments.clone();
        let client_for_pong = client.clone();

        tokio::spawn(async move {
            while let Some(msg) = raw_rx.recv().await {
                let text = match msg {
                    Message::Text(t) => t.to_string(),
                    Message::Binary(b) => match String::from_utf8(b.to_vec()) {
                        Ok(s) => s,
                        Err(_) => continue,
                    },
                    Message::Close(_) => break,
                    _ => continue,
                };

                // Application-level ping/pong keepalive.
                if let Ok(ping) = serde_json::from_str::<HashKeyWsPing>(&text) {
                    let pong = format!("{{\"pong\":{}}}", ping.ping);
                    if let Err(e) = client_for_pong.send_text(pong, None).await {
                        log::debug!("Failed to send HashKey pong: {e}");
                    }
                    continue;
                }

                let push: HashKeyWsPush = match serde_json::from_str(&text) {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                let Some(entry) = instruments.get(&Ustr::from(push.symbol.as_str())) else {
                    log::debug!("No cached instrument for HashKey push: {}", push.symbol);
                    continue;
                };
                let (instrument_id, price_precision, size_precision) = *entry;
                let ts_init = UnixNanos::default();

                let data = Self::parse_push(
                    &push,
                    instrument_id,
                    price_precision,
                    size_precision,
                    ts_init,
                );
                if !data.is_empty() && out_tx.send(HashKeyWsMessage::Data(data)).is_err() {
                    break;
                }
            }
        });

        self.inner = Some(client);
        self.out_rx = Some(out_rx);
        Ok(())
    }

    fn parse_push(
        push: &HashKeyWsPush,
        instrument_id: InstrumentId,
        price_precision: u8,
        size_precision: u8,
        ts_init: UnixNanos,
    ) -> Vec<Data> {
        let mut out = Vec::new();
        match push.topic.as_str() {
            "depth" => {
                if let Some(value) = push.data.first()
                    && let Ok(depth) = serde_json::from_value::<HashKeyWsDepth>(value.clone())
                {
                    match parse_ws_depth(
                        &depth,
                        instrument_id,
                        price_precision,
                        size_precision,
                        ts_init,
                    ) {
                        Ok(deltas) => out.push(Data::Deltas(
                            nautilus_model::data::OrderBookDeltas_API::new(deltas),
                        )),
                        Err(e) => log::error!("Failed to parse HashKey depth: {e}"),
                    }
                }
            }
            "trade" => {
                for value in &push.data {
                    if let Ok(trade) = serde_json::from_value::<HashKeyWsTrade>(value.clone()) {
                        match parse_ws_trade(
                            &trade,
                            instrument_id,
                            price_precision,
                            size_precision,
                            ts_init,
                        ) {
                            Ok(tick) => out.push(Data::Trade(tick)),
                            Err(e) => log::error!("Failed to parse HashKey trade: {e}"),
                        }
                    }
                }
            }
            other => log::debug!("Ignoring HashKey topic: {other}"),
        }
        out
    }

    /// Subscribes to the order book (depth) stream for `symbol` (raw id, e.g. `BTCUSDT`).
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or the send fails.
    pub async fn subscribe_book(&self, symbol: &str) -> anyhow::Result<()> {
        self.subscribe(symbol, "depth").await
    }

    /// Subscribes to the trade stream for `symbol`.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or the send fails.
    pub async fn subscribe_trades(&self, symbol: &str) -> anyhow::Result<()> {
        self.subscribe(symbol, "trade").await
    }

    async fn subscribe(&self, symbol: &str, topic: &str) -> anyhow::Result<()> {
        let client = self
            .inner
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("HashKey websocket not connected"))?;
        let msg = format!(
            "{{\"symbol\":\"{symbol}\",\"topic\":\"{topic}\",\"event\":\"sub\"}}"
        );
        client.send_text(msg, None).await?;
        Ok(())
    }

    /// Waits until the connection is active, polling up to `timeout_secs`.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection does not become active in time.
    pub async fn wait_until_active(&self, timeout_secs: f64) -> anyhow::Result<()> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs_f64(timeout_secs);
        while tokio::time::Instant::now() < deadline {
            if self.is_active() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        anyhow::bail!("HashKey websocket did not become active within {timeout_secs}s")
    }

    /// Takes the internal message stream. Can only be taken once per connection.
    pub fn stream(&mut self) -> impl Stream<Item = HashKeyWsMessage> + 'static {
        let mut rx = self.out_rx.take();
        async_stream::stream! {
            if let Some(rx) = rx.as_mut() {
                while let Some(msg) = rx.recv().await {
                    yield msg;
                }
            }
        }
    }

    /// Disconnects the underlying WebSocket.
    pub async fn disconnect(&mut self) {
        if let Some(client) = self.inner.take() {
            client.disconnect().await;
        }
    }
}
