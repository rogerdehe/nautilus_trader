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

//! WebSocket client for the Gate (gate.io) APIv4 spot streams.
//!
//! Subscribes to the `spot.order_book` full-snapshot channel (Clear+Add deltas) and `spot.trades`,
//! per CCXT pro `gate.py`. Transport keep-alive uses protocol ping/pong (handled by the transport);
//! reconnection + subscription replay are handled by [`WebSocketClient`] in handler mode.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use nautilus_core::{AtomicMap, time::get_atomic_clock_realtime};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::{WebSocketClient, WebSocketConfig, channel_message_handler};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_tungstenite::tungstenite::Message;

use crate::{
    common::{
        consts::{CH_SPOT_ORDER_BOOK, CH_SPOT_TRADES, GATE_WS_BOOK_INTERVAL, GATE_WS_BOOK_LEVELS},
        parse::gate_symbol_from_instrument_id,
    },
    websocket::messages::WsRequest,
};

/// WebSocket heartbeat interval (seconds) — sends transport ping frames.
const WS_HEARTBEAT_SECS: u64 = 15;

/// WebSocket client for Gate spot market data.
///
/// Cheaply cloneable: clones share the transport (`inner`), instrument cache and request-id
/// counter, but only the original retains the inbound receiver (`raw_rx`). This lets the data
/// client keep a clone for issuing subscribes while a dedicated task drains the receiver.
pub struct GateWebSocketClient {
    url: String,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    inner: Option<Arc<WebSocketClient>>,
    raw_rx: Option<UnboundedReceiver<Message>>,
    req_id: Arc<AtomicU64>,
}

impl Clone for GateWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            instruments: Arc::clone(&self.instruments),
            inner: self.inner.clone(),
            raw_rx: None,
            req_id: Arc::clone(&self.req_id),
        }
    }
}

impl std::fmt::Debug for GateWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(GateWebSocketClient))
            .field("url", &self.url)
            .field("connected", &self.inner.is_some())
            .finish()
    }
}

impl GateWebSocketClient {
    /// Creates a new client for the given WS URL (see [`crate::common::consts::GATE_SPOT_WS_URL`]).
    #[must_use]
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            instruments: Arc::new(AtomicMap::new()),
            inner: None,
            raw_rx: None,
            req_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Returns the instrument cache shared with the parse path.
    #[must_use]
    pub fn instruments(&self) -> Arc<AtomicMap<InstrumentId, InstrumentAny>> {
        Arc::clone(&self.instruments)
    }

    /// Populates the instrument cache used to resolve precisions for inbound messages.
    pub fn initialize_instruments(&self, instruments: Vec<InstrumentAny>) {
        for instrument in instruments {
            self.instruments.insert(instrument.id(), instrument);
        }
    }

    /// Returns `true` when the client holds a live connection.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.inner.as_ref().is_some_and(|c| c.is_active())
    }

    fn config(&self) -> WebSocketConfig {
        WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat: Some(WS_HEARTBEAT_SECS),
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(10_000),
            reconnect_delay_initial_ms: Some(1_000),
            reconnect_delay_max_ms: Some(30_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(250),
            reconnect_max_attempts: None,
            idle_timeout_ms: Some(60_000),
            backend: Default::default(),
            proxy_url: None,
        }
    }

    /// Establishes the WebSocket connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let (handler, raw_rx) = channel_message_handler();
        let client =
            WebSocketClient::connect(self.config(), Some(handler), None, None, vec![], None).await?;
        self.inner = Some(Arc::new(client));
        self.raw_rx = Some(raw_rx);
        Ok(())
    }

    fn next_id(&self) -> u64 {
        self.req_id.fetch_add(1, Ordering::Relaxed)
    }

    async fn send_request(&self, req: &WsRequest) -> anyhow::Result<()> {
        let client = self
            .inner
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Gate WS not connected"))?;
        let json = serde_json::to_string(req)?;
        client
            .send_text(json, None)
            .await
            .map_err(|e| anyhow::anyhow!("failed to send Gate WS request: {e}"))
    }

    fn build_request(&self, channel: &str, payload: Vec<String>) -> WsRequest {
        WsRequest {
            id: self.next_id(),
            time: (get_atomic_clock_realtime().get_time_ns().as_u64() / 1_000_000_000) as i64,
            channel: channel.to_string(),
            event: "subscribe".to_string(),
            payload,
        }
    }

    /// Subscribes to the `spot.order_book` snapshot channel for `instrument_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or the send fails.
    pub async fn subscribe_book(&self, instrument_id: &InstrumentId) -> anyhow::Result<()> {
        let market_id = gate_symbol_from_instrument_id(instrument_id);
        let payload = vec![
            market_id,
            GATE_WS_BOOK_LEVELS.to_string(),
            GATE_WS_BOOK_INTERVAL.to_string(),
        ];
        let req = self.build_request(CH_SPOT_ORDER_BOOK, payload);
        self.send_request(&req).await
    }

    /// Subscribes to the `spot.trades` channel for `instrument_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or the send fails.
    pub async fn subscribe_trades(&self, instrument_id: &InstrumentId) -> anyhow::Result<()> {
        let market_id = gate_symbol_from_instrument_id(instrument_id);
        let req = self.build_request(CH_SPOT_TRADES, vec![market_id]);
        self.send_request(&req).await
    }

    /// Takes ownership of the raw inbound message receiver (available after [`Self::connect`]).
    ///
    /// The caller drives the receiver in a task, decoding each text frame with
    /// [`crate::websocket::parse::parse_ws_message`] (using [`Self::instruments`] for precision
    /// lookups) and forwarding the parsed data. Returns `None` if already taken or not connected.
    #[must_use]
    pub fn take_receiver(&mut self) -> Option<UnboundedReceiver<Message>> {
        self.raw_rx.take()
    }

    /// Disconnects the underlying transport, if connected.
    pub async fn disconnect(&mut self) {
        if let Some(client) = self.inner.take() {
            client.disconnect().await;
        }
        self.raw_rx = None;
    }
}
