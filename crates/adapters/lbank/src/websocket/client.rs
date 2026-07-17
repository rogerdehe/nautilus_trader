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

//! WebSocket client for the LBank spot streams (`wss://www.lbkex.net/ws/V2/`).
//!
//! Subscribes to the full-snapshot `depth` channel (Clear+Add deltas) and `trade`, per CCXT pro
//! `lbank.py`. LBank's keepalive is a MANDATORY APP-LEVEL ping: the server sends
//! `{"action":"ping","ping":"<uuid>"}` and the client MUST reply `{"action":"pong","pong":"<uuid>"}`
//! within ~60s. That reply is driven by the data-client read loop via [`Self::send_pong`]; the
//! transport `heartbeat` is a separate, lower-level keepalive and does NOT satisfy it.

use std::sync::Arc;

use nautilus_core::AtomicMap;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::{WebSocketClient, WebSocketConfig, channel_message_handler};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_tungstenite::tungstenite::Message;

use crate::{
    common::{
        consts::{
            WS_ACTION_PONG, WS_ACTION_SUBSCRIBE, WS_DEPTH_LEVELS, WS_SUBSCRIBE_DEPTH,
            WS_SUBSCRIBE_TRADE,
        },
        parse::lbank_symbol_from_instrument_id,
    },
    websocket::messages::{WsPong, WsSubscribeRequest},
};

/// WebSocket heartbeat interval (seconds) — transport-level ping frames (separate from the app
/// ping/pong keepalive that LBank requires).
const WS_HEARTBEAT_SECS: u64 = 15;

/// WebSocket client for LBank spot market data.
///
/// Cheaply cloneable: clones share the transport (`inner`) and instrument cache, but only the
/// original retains the inbound receiver (`raw_rx`). A clone is handed to the read loop so it can
/// issue pongs while the original drains the receiver.
pub struct LbankWebSocketClient {
    url: String,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    inner: Option<Arc<WebSocketClient>>,
    raw_rx: Option<UnboundedReceiver<Message>>,
}

impl Clone for LbankWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            instruments: Arc::clone(&self.instruments),
            inner: self.inner.clone(),
            raw_rx: None,
        }
    }
}

impl std::fmt::Debug for LbankWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(LbankWebSocketClient))
            .field("url", &self.url)
            .field("connected", &self.inner.is_some())
            .finish()
    }
}

impl LbankWebSocketClient {
    /// Creates a new client for the given WS URL (see [`crate::common::consts::LBANK_SPOT_WS_URL`]).
    #[must_use]
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            instruments: Arc::new(AtomicMap::new()),
            inner: None,
            raw_rx: None,
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

    async fn send_text(&self, json: String) -> anyhow::Result<()> {
        let client = self
            .inner
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("LBank WS not connected"))?;
        client
            .send_text(json, None)
            .await
            .map_err(|e| anyhow::anyhow!("failed to send LBank WS text: {e}"))
    }

    /// Subscribes to the full-snapshot `depth` channel for `instrument_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or the send fails.
    pub async fn subscribe_book(&self, instrument_id: &InstrumentId) -> anyhow::Result<()> {
        let req = WsSubscribeRequest {
            action: WS_ACTION_SUBSCRIBE.to_string(),
            subscribe: WS_SUBSCRIBE_DEPTH.to_string(),
            depth: Some(WS_DEPTH_LEVELS),
            pair: lbank_symbol_from_instrument_id(instrument_id),
        };
        self.send_text(serde_json::to_string(&req)?).await
    }

    /// Subscribes to the `trade` channel for `instrument_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or the send fails.
    pub async fn subscribe_trades(&self, instrument_id: &InstrumentId) -> anyhow::Result<()> {
        let req = WsSubscribeRequest {
            action: WS_ACTION_SUBSCRIBE.to_string(),
            subscribe: WS_SUBSCRIBE_TRADE.to_string(),
            depth: None,
            pair: lbank_symbol_from_instrument_id(instrument_id),
        };
        self.send_text(serde_json::to_string(&req)?).await
    }

    /// Replies to the mandatory app-level ping with `{"action":"pong","pong":"<uuid>"}`.
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or the send fails.
    pub async fn send_pong(&self, ping_id: &str) -> anyhow::Result<()> {
        let pong = WsPong {
            action: WS_ACTION_PONG.to_string(),
            pong: ping_id.to_string(),
        };
        self.send_text(serde_json::to_string(&pong)?).await
    }

    /// Takes ownership of the raw inbound message receiver (available after [`Self::connect`]).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribe_book_message_shape() {
        let id = InstrumentId::from("BTC_USDT.LBANK");
        let req = WsSubscribeRequest {
            action: WS_ACTION_SUBSCRIBE.to_string(),
            subscribe: WS_SUBSCRIBE_DEPTH.to_string(),
            depth: Some(WS_DEPTH_LEVELS),
            pair: lbank_symbol_from_instrument_id(&id),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""action":"subscribe""#));
        assert!(json.contains(r#""subscribe":"depth""#));
        assert!(json.contains(r#""depth":100"#));
        assert!(json.contains(r#""pair":"btc_usdt""#));
    }

    #[test]
    fn trade_subscribe_omits_depth() {
        let id = InstrumentId::from("ETH_USDT.LBANK");
        let req = WsSubscribeRequest {
            action: WS_ACTION_SUBSCRIBE.to_string(),
            subscribe: WS_SUBSCRIBE_TRADE.to_string(),
            depth: None,
            pair: lbank_symbol_from_instrument_id(&id),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("depth"));
        assert!(json.contains(r#""pair":"eth_usdt""#));
    }

    #[test]
    fn pong_message_shape() {
        let pong = WsPong {
            action: WS_ACTION_PONG.to_string(),
            pong: "uuid-123".to_string(),
        };
        let json = serde_json::to_string(&pong).unwrap();
        assert_eq!(json, r#"{"action":"pong","pong":"uuid-123"}"#);
    }
}
