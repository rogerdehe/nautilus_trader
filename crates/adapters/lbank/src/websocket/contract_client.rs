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
//! ## One connection per instrument (hard requirement)
//!
//! LBank's contract v3 stream sustains live push for **exactly one instrument per socket**: pile
//! many subscriptions on one connection and the server silently keeps only the last-subscribed
//! instrument streaming, delivering every other sub just its opening snapshot then going quiet
//! (verified empirically — N=2..48 on one socket → 1 survivor, no error frame). So this client opens
//! a **dedicated socket per instrument**, each carrying that coin's OrderBook (depth) + Deal (trade)
//! channels. Attribution is by connection (the socket *is* the coin) — depth frames carry no symbol
//! but need none.
//!
//! Two per-connection concerns:
//! - **Keepalive**: an ACTIVE client ping (`{"action":"ping","ping":"<ms>"}` every
//!   [`CONTRACT_WS_PING_SECS`]); the server drops the socket without it. The ping task holds the
//!   `Arc<WebSocketClient>`, which survives reconnects (the transport swaps its reader internally).
//! - **Reconnect resubscribe**: a reconnected socket has no active subscription, so the coin would go
//!   silently dead. The `post_reconnection` hook replays this connection's subscribe messages.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use nautilus_common::live::runtime::get_runtime;
use nautilus_core::AtomicMap;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::{WebSocketClient, WebSocketConfig, channel_message_handler};
use tokio::sync::{Mutex as AsyncMutex, mpsc};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::{
    common::consts::{CONTRACT_WS_DEPTH_LIMIT, CONTRACT_WS_PING_SECS},
    websocket::contract_messages::{ContractWsPing, ContractWsSubscribe},
};

/// Transport-level heartbeat (seconds) — separate from the app-level ping keepalive.
const WS_HEARTBEAT_SECS: u64 = 15;
/// Cap on a single socket's connect handshake so one unreachable instrument can't wedge the
/// connection lock and block every other subscription.
const CONNECT_TIMEOUT_SECS: u64 = 20;
/// Minimum spacing between new-socket handshakes. Opening ~49 sockets in a burst trips Cloudflare's
/// per-IP handshake rate limit (`429 Too Many Requests`); connection creation is serialized by the
/// `conns` lock, so a pre-connect sleep paces every handshake to stay under the limit.
const CONNECT_STAGGER_MS: u64 = 500;
/// Base backoff between handshake retries (doubles per attempt) when a handshake is rejected (429).
const CONNECT_BACKOFF_MS: u64 = 1_000;
/// Handshake attempts per socket before giving up (so a transient 429 doesn't permanently drop a coin).
const CONNECT_RETRIES: u32 = 5;

/// A single instrument's dedicated socket: the transport, its replayable subscribe messages, and the
/// cancellation tokens for its ping + forward tasks.
struct InstrumentConn {
    client: Arc<WebSocketClient>,
    /// Subscribe payloads to (re)send — one per channel (depth, trades). Replayed on reconnect.
    subs: Arc<Mutex<Vec<String>>>,
    ping_token: CancellationToken,
    forward_token: CancellationToken,
}

/// WebSocket client for LBank contract market data — owns one socket per subscribed instrument.
pub struct LbankContractWebSocketClient {
    url: String,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    /// One dedicated connection per instrument (LBank streams one instrument per socket).
    conns: Arc<AsyncMutex<HashMap<InstrumentId, InstrumentConn>>>,
    /// Merged inbound stream: every connection's messages, tagged with their instrument.
    out_tx: mpsc::UnboundedSender<(InstrumentId, Message)>,
    out_rx: Option<mpsc::UnboundedReceiver<(InstrumentId, Message)>>,
}

impl Clone for LbankContractWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            instruments: Arc::clone(&self.instruments),
            conns: Arc::clone(&self.conns),
            out_tx: self.out_tx.clone(),
            // Only the original holds the receiver.
            out_rx: None,
        }
    }
}

impl std::fmt::Debug for LbankContractWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(LbankContractWebSocketClient))
            .field("url", &self.url)
            .finish()
    }
}

impl LbankContractWebSocketClient {
    /// Creates a new client for the given v3 WS URL.
    #[must_use]
    pub fn new(url: &str) -> Self {
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        Self {
            url: url.to_string(),
            instruments: Arc::new(AtomicMap::new()),
            conns: Arc::new(AsyncMutex::new(HashMap::new())),
            out_tx,
            out_rx: Some(out_rx),
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

    /// No-op retained for API symmetry — connections are established lazily per instrument on the
    /// first subscribe. Kept so callers can `connect().await` before `take_receiver()`.
    ///
    /// # Errors
    ///
    /// Never errors.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        Ok(())
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

    /// Ensures a dedicated socket exists for `instrument_id`, creating + wiring it (ping task, forward
    /// task, reconnect-resubscribe hook) on first use. Idempotent: a second call for the same
    /// instrument reuses the existing socket (so depth + trades share one connection).
    ///
    /// Returns the shared subscribe-message list for the connection, into which the caller pushes its
    /// channel's payload (so it replays on reconnect).
    async fn ensure_conn(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<(Arc<WebSocketClient>, Arc<Mutex<Vec<String>>>)> {
        let mut conns = self.conns.lock().await;
        if let Some(existing) = conns.get(&instrument_id) {
            return Ok((Arc::clone(&existing.client), Arc::clone(&existing.subs)));
        }

        let subs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        // Reconnect-resubscribe: replay this connection's subscribes after a reconnected socket comes
        // back with no active subscription (else the coin goes silently dead).
        let slot: Arc<OnceLock<Arc<WebSocketClient>>> = Arc::new(OnceLock::new());
        let post_reconnection = {
            let slot = Arc::clone(&slot);
            let subs = Arc::clone(&subs);
            Arc::new(move || {
                let Some(client) = slot.get().cloned() else { return };
                let payloads: Vec<String> = subs.lock().map(|v| v.clone()).unwrap_or_default();
                if payloads.is_empty() {
                    return;
                }
                get_runtime().spawn(async move {
                    for p in payloads {
                        if let Err(e) = client.send_text(p, None).await {
                            log::debug!("LBank contract resubscribe send failed: {e}");
                        }
                    }
                });
            }) as Arc<dyn Fn() + Send + Sync>
        };

        // Staggered, retrying handshake: paced by the sleep below (the `conns` lock serializes new
        // connections), with backoff retries so a transient Cloudflare 429 doesn't drop the coin.
        let mut established: Option<(WebSocketClient, mpsc::UnboundedReceiver<Message>)> = None;
        let mut last_err = String::new();
        for attempt in 0..CONNECT_RETRIES {
            let delay = if attempt == 0 {
                CONNECT_STAGGER_MS
            } else {
                CONNECT_BACKOFF_MS.saturating_mul(1u64 << (attempt - 1))
            };
            tokio::time::sleep(Duration::from_millis(delay)).await;
            let (handler, raw_rx) = channel_message_handler();
            let connect_fut = WebSocketClient::connect(
                self.config(),
                Some(handler),
                None,
                Some(Arc::clone(&post_reconnection)),
                vec![],
                None,
            );
            match tokio::time::timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS), connect_fut).await {
                Ok(Ok(c)) => {
                    established = Some((c, raw_rx));
                    break;
                }
                Ok(Err(e)) => last_err = e.to_string(),
                Err(_) => last_err = "connect timed out".to_string(),
            }
        }
        let (client, mut raw_rx) = established.ok_or_else(|| {
            anyhow::anyhow!(
                "LBank contract WS connect failed for {instrument_id} after {CONNECT_RETRIES} attempts: {last_err}"
            )
        })?;
        let client = Arc::new(client);
        let _ = slot.set(Arc::clone(&client));

        // ACTIVE keepalive per socket (the v3 server closes without a periodic app-level ping).
        let ping_token = CancellationToken::new();
        {
            let token = ping_token.clone();
            let ping_client = Arc::clone(&client);
            get_runtime().spawn(async move {
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
        }

        // Forward this socket's inbound messages into the merged stream, tagged with the instrument.
        let forward_token = CancellationToken::new();
        {
            let token = forward_token.clone();
            let out_tx = self.out_tx.clone();
            let iid = instrument_id;
            get_runtime().spawn(async move {
                loop {
                    tokio::select! {
                        () = token.cancelled() => break,
                        msg = raw_rx.recv() => {
                            let Some(msg) = msg else { break };
                            if out_tx.send((iid, msg)).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }

        conns.insert(
            instrument_id,
            InstrumentConn {
                client: Arc::clone(&client),
                subs: Arc::clone(&subs),
                ping_token,
                forward_token,
            },
        );
        Ok((client, subs))
    }

    /// Subscribes to the OrderBook (depth) channel for `symbol` merged at `price_tick` (the
    /// instrument's native tick string, e.g. `"0.1"`) with [`CONTRACT_WS_DEPTH_LIMIT`] levels, on the
    /// instrument's dedicated socket (created on first use).
    ///
    /// # Errors
    ///
    /// Returns an error if the socket cannot be established or the send fails.
    pub async fn subscribe_book(
        &self,
        instrument_id: InstrumentId,
        symbol: &str,
        price_tick: &str,
    ) -> anyhow::Result<()> {
        let (client, subs) = self.ensure_conn(instrument_id).await?;
        // sub_id (`y`) is irrelevant now that attribution is per-connection; use a stable per-channel
        // value (1 = depth, 2 = trades) so the shapes match what the server echoes.
        let req = ContractWsSubscribe::order_book(symbol, price_tick, CONTRACT_WS_DEPTH_LIMIT, 1);
        let json = serde_json::to_string(&req)?;
        if let Ok(mut v) = subs.lock() {
            v.push(json.clone());
        }
        client
            .send_text(json, None)
            .await
            .map_err(|e| anyhow::anyhow!("failed to send LBank contract book subscribe: {e}"))
    }

    /// Subscribes to the Deal (trades) channel for `symbol` (bare symbol id), on the instrument's
    /// dedicated socket (reused if the book was already subscribed).
    ///
    /// # Errors
    ///
    /// Returns an error if the socket cannot be established or the send fails.
    pub async fn subscribe_trades(
        &self,
        instrument_id: InstrumentId,
        symbol: &str,
    ) -> anyhow::Result<()> {
        let (client, subs) = self.ensure_conn(instrument_id).await?;
        let req = ContractWsSubscribe::deal(symbol, 2);
        let json = serde_json::to_string(&req)?;
        if let Ok(mut v) = subs.lock() {
            v.push(json.clone());
        }
        client
            .send_text(json, None)
            .await
            .map_err(|e| anyhow::anyhow!("failed to send LBank contract trades subscribe: {e}"))
    }

    /// Takes ownership of the merged inbound receiver (available on the original client instance).
    #[must_use]
    pub fn take_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<(InstrumentId, Message)>> {
        self.out_rx.take()
    }

    /// Disconnects every per-instrument socket and stops their ping + forward tasks.
    pub async fn disconnect(&mut self) {
        let mut conns = self.conns.lock().await;
        for (_iid, conn) in conns.drain() {
            conn.ping_token.cancel();
            conn.forward_token.cancel();
            conn.client.disconnect().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_subscribe_message_shape() {
        let sub = ContractWsSubscribe::order_book("BTCUSDT", "0.1", CONTRACT_WS_DEPTH_LIMIT, 1);
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
    fn new_client_holds_receiver() {
        let mut c = LbankContractWebSocketClient::new("wss://x/ws/v3");
        assert!(c.take_receiver().is_some());
        // Only one receiver exists.
        assert!(c.take_receiver().is_none());
    }
}
