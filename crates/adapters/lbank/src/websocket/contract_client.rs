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

//! WebSocket client for the LBank CONTRACT (USDT-perp) v3 market-data stream (`cfdWsUrl`).
//!
//! Unlike spot, keepalive is an ACTIVE client ping: a background task sends
//! `{"action":"ping","ping":"<ms>"}` every [`CONTRACT_WS_PING_SECS`] or the server drops the socket.
//! Subscribes to OrderBook (composite `symbol_decimal_limit` id) and Deal (bare symbol).

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use nautilus_common::live::runtime::get_runtime;
use nautilus_core::AtomicMap;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::{WebSocketClient, WebSocketConfig, channel_message_handler};
use tokio::{sync::mpsc::UnboundedReceiver, task::JoinHandle};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::{
    common::consts::{CONTRACT_WS_DEPTH_LIMIT, CONTRACT_WS_PING_SECS},
    websocket::contract_messages::{ContractWsPing, ContractWsSubscribe},
};

/// Transport-level heartbeat (seconds) — separate from the app-level ping keepalive.
const WS_HEARTBEAT_SECS: u64 = 15;

/// WebSocket client for LBank contract market data.
pub struct LbankContractWebSocketClient {
    url: String,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    /// Maps our subscription id (`y`) to the instrument — the only way to attribute a depth push,
    /// which carries no symbol.
    sub_map: Arc<AtomicMap<u64, InstrumentId>>,
    inner: Option<Arc<WebSocketClient>>,
    raw_rx: Option<UnboundedReceiver<Message>>,
    sub_counter: Arc<AtomicU64>,
    ping_token: CancellationToken,
    ping_task: Option<Arc<JoinHandle<()>>>,
}

impl Clone for LbankContractWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            instruments: Arc::clone(&self.instruments),
            sub_map: Arc::clone(&self.sub_map),
            inner: self.inner.clone(),
            raw_rx: None,
            sub_counter: Arc::clone(&self.sub_counter),
            ping_token: self.ping_token.clone(),
            ping_task: self.ping_task.clone(),
        }
    }
}

impl std::fmt::Debug for LbankContractWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(LbankContractWebSocketClient))
            .field("url", &self.url)
            .field("connected", &self.inner.is_some())
            .finish()
    }
}

impl LbankContractWebSocketClient {
    /// Creates a new client for the given v3 WS URL.
    #[must_use]
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            instruments: Arc::new(AtomicMap::new()),
            sub_map: Arc::new(AtomicMap::new()),
            inner: None,
            raw_rx: None,
            sub_counter: Arc::new(AtomicU64::new(1)),
            ping_token: CancellationToken::new(),
            ping_task: None,
        }
    }

    /// Returns the subscription-id → instrument map shared with the parse loop.
    #[must_use]
    pub fn sub_map(&self) -> Arc<AtomicMap<u64, InstrumentId>> {
        Arc::clone(&self.sub_map)
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
            // The obfuscated prod host validates the browser Origin.
            headers: vec![("Origin".to_string(), "https://www.lbank.com".to_string())],
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

    /// Establishes the WebSocket connection and starts the mandatory app-level ping task.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let (handler, raw_rx) = channel_message_handler();
        let client =
            WebSocketClient::connect(self.config(), Some(handler), None, None, vec![], None).await?;
        let client = Arc::new(client);
        self.inner = Some(Arc::clone(&client));
        self.raw_rx = Some(raw_rx);

        // ACTIVE keepalive: the v3 server closes the socket without a periodic app-level ping.
        self.ping_token = CancellationToken::new();
        let token = self.ping_token.clone();
        let ping_client = Arc::clone(&client);
        let task = get_runtime().spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(CONTRACT_WS_PING_SECS));
            loop {
                tokio::select! {
                    () = token.cancelled() => break,
                    _ = interval.tick() => {
                        let ms = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_millis())
                            .unwrap_or(0);
                        let ping = ContractWsPing { action: "ping".to_string(), ping: ms.to_string() };
                        match serde_json::to_string(&ping) {
                            Ok(txt) => {
                                if let Err(e) = ping_client.send_text(txt, None).await {
                                    log::debug!("LBank contract ping failed: {e}");
                                }
                            }
                            Err(e) => log::error!("LBank contract ping serialize failed: {e}"),
                        }
                    }
                }
            }
        });
        self.ping_task = Some(Arc::new(task));
        Ok(())
    }

    async fn send_text(&self, json: String) -> anyhow::Result<()> {
        let client = self
            .inner
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("LBank contract WS not connected"))?;
        client
            .send_text(json, None)
            .await
            .map_err(|e| anyhow::anyhow!("failed to send LBank contract WS text: {e}"))
    }

    fn next_sub_id(&self) -> u64 {
        self.sub_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Subscribes to the OrderBook (depth) channel for `symbol` merged at `price_tick` (the
    /// instrument's native tick string, e.g. `"0.1"`) with [`CONTRACT_WS_DEPTH_LIMIT`] levels.
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or the send fails.
    pub async fn subscribe_book(
        &self,
        instrument_id: InstrumentId,
        symbol: &str,
        price_tick: &str,
    ) -> anyhow::Result<()> {
        let sub_id = self.next_sub_id();
        self.sub_map.insert(sub_id, instrument_id);
        let req = ContractWsSubscribe::order_book(symbol, price_tick, CONTRACT_WS_DEPTH_LIMIT, sub_id);
        self.send_text(serde_json::to_string(&req)?).await
    }

    /// Subscribes to the Deal (trades) channel for `symbol` (bare symbol id).
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or the send fails.
    pub async fn subscribe_trades(
        &self,
        instrument_id: InstrumentId,
        symbol: &str,
    ) -> anyhow::Result<()> {
        let sub_id = self.next_sub_id();
        self.sub_map.insert(sub_id, instrument_id);
        let req = ContractWsSubscribe::deal(symbol, sub_id);
        self.send_text(serde_json::to_string(&req)?).await
    }

    /// Takes ownership of the raw inbound message receiver (available after [`Self::connect`]).
    #[must_use]
    pub fn take_receiver(&mut self) -> Option<UnboundedReceiver<Message>> {
        self.raw_rx.take()
    }

    /// Disconnects the underlying transport and stops the ping task, if connected.
    pub async fn disconnect(&mut self) {
        self.ping_token.cancel();
        self.ping_task = None;
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
    fn book_subscribe_message_shape() {
        let sub = ContractWsSubscribe::order_book("BTCUSDT", "0.1", CONTRACT_WS_DEPTH_LIMIT, 3);
        let json = serde_json::to_string(&sub).unwrap();
        assert!(json.contains(r#""x":3"#));
        assert!(json.contains(r#""i":"BTCUSDT_0.1_25""#));
    }

    #[test]
    fn ping_message_shape() {
        let ping = ContractWsPing { action: "ping".to_string(), ping: "1785662436070".to_string() };
        let json = serde_json::to_string(&ping).unwrap();
        assert_eq!(json, r#"{"action":"ping","ping":"1785662436070"}"#);
    }

    #[test]
    fn sub_ids_increment() {
        let c = LbankContractWebSocketClient::new("wss://x/ws/v3");
        assert_eq!(c.next_sub_id(), 1);
        assert_eq!(c.next_sub_id(), 2);
    }
}
