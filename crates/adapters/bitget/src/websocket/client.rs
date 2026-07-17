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

//! WebSocket client for the Bitget v2 public market-data streams.
//!
//! Connects to `wss://ws.bitget.com/v2/ws/public`, subscribes to depth (`books15` snapshot) and
//! trade channels, keeps the socket alive with the app-level `ping`/`pong` protocol (CCXT
//! `pro/bitget.py::ping`), parses pushes into [`Data`], and forwards them over an unbounded channel.

use std::sync::Arc;

use dashmap::DashMap;
use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    identifiers::InstrumentId,
};
use nautilus_network::{
    ratelimiter::quota::Quota,
    websocket::{
        WebSocketClient, WebSocketConfig, channel_message_handler,
    },
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;

use super::{
    messages::{
        BITGET_BOOK_CHANNEL, BITGET_TRADE_CHANNEL, BitgetWsBook, BitgetWsPush, BitgetWsRequest,
        BitgetWsTrade,
    },
    parse::{parse_book_snapshot, parse_ws_trade, product_from_inst_type},
};
use crate::common::{
    consts::{BITGET_WS_HEARTBEAT_SECS, BITGET_WS_PING, BITGET_WS_PONG, BITGET_WS_PUBLIC_URL},
    enums::BitgetProductType,
    parse::instrument_id_from_raw,
};

/// Instrument type string for a product used in subscription args.
fn inst_type_str(product: BitgetProductType) -> String {
    product.to_string()
}

/// WebSocket client for Bitget public market-data streams.
pub struct BitgetWebSocketClient {
    url: String,
    inner: Option<Arc<WebSocketClient>>,
    /// Per-instrument (price_precision, size_precision) required to parse pushes.
    precisions: Arc<DashMap<InstrumentId, (u8, u8)>>,
    data_tx: UnboundedSender<Data>,
    data_rx: Option<UnboundedReceiver<Data>>,
}

impl std::fmt::Debug for BitgetWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BitgetWebSocketClient")
            .field("url", &self.url)
            .field("connected", &self.inner.is_some())
            .finish()
    }
}

impl BitgetWebSocketClient {
    /// Creates a new [`BitgetWebSocketClient`] for the public stream.
    #[must_use]
    pub fn new(url: Option<String>) -> Self {
        let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            url: url.unwrap_or_else(|| BITGET_WS_PUBLIC_URL.to_string()),
            inner: None,
            precisions: Arc::new(DashMap::new()),
            data_tx,
            data_rx: Some(data_rx),
        }
    }

    /// Returns `true` if the underlying socket is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.inner.as_ref().is_some_and(|c| c.is_active())
    }

    /// Registers an instrument's price/size precision so its pushes can be parsed.
    pub fn add_instrument(&self, instrument_id: InstrumentId, price_precision: u8, size_precision: u8) {
        self.precisions
            .insert(instrument_id, (price_precision, size_precision));
    }

    /// Returns a clone of the shared precision map (shared with the read loop).
    #[must_use]
    pub fn precisions(&self) -> Arc<DashMap<InstrumentId, (u8, u8)>> {
        self.precisions.clone()
    }

    /// Returns a clone of the underlying connected socket handle, if connected.
    ///
    /// Callers use this to send subscription frames from synchronous contexts by spawning a task.
    #[must_use]
    pub fn inner_handle(&self) -> Option<Arc<WebSocketClient>> {
        self.inner.clone()
    }

    /// Builds the JSON `subscribe` frame text for an instrument on a channel.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn subscription_text(instrument_id: InstrumentId, channel: &str) -> anyhow::Result<String> {
        let (raw_symbol, is_perp) =
            crate::common::parse::raw_symbol_from_instrument_id(&instrument_id);
        let product = if is_perp {
            BitgetProductType::UsdtFutures
        } else {
            BitgetProductType::Spot
        };
        let request = BitgetWsRequest::subscribe(&inst_type_str(product), channel, &raw_symbol);
        Ok(serde_json::to_string(&request)?)
    }

    /// Takes the receiver end of the parsed-data stream (callable once).
    pub fn take_stream(&mut self) -> Option<UnboundedReceiver<Data>> {
        self.data_rx.take()
    }

    /// Connects to the Bitget public WebSocket and starts the read loop.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let (message_handler, raw_rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat: Some(BITGET_WS_HEARTBEAT_SECS),
            heartbeat_msg: Some(BITGET_WS_PING.to_string()),
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: None,
            reconnect_delay_max_ms: None,
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: Default::default(),
            proxy_url: None,
        };

        let quota = Quota::per_second(std::num::NonZeroU32::new(10).expect("non-zero"))
            .expect("valid quota");

        let client = WebSocketClient::connect(
            config,
            Some(message_handler),
            None,
            None,
            vec![],
            Some(quota),
        )
        .await?;

        self.inner = Some(Arc::new(client));

        let precisions = self.precisions.clone();
        let data_tx = self.data_tx.clone();
        tokio::spawn(async move {
            Self::read_loop(raw_rx, precisions, data_tx).await;
        });

        Ok(())
    }

    /// Subscribes to the order book (`books15` snapshot) channel for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or the send fails.
    pub async fn subscribe_book(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        self.send_subscription(instrument_id, BITGET_BOOK_CHANNEL).await
    }

    /// Subscribes to the trade channel for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or the send fails.
    pub async fn subscribe_trades(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        self.send_subscription(instrument_id, BITGET_TRADE_CHANNEL).await
    }

    async fn send_subscription(
        &self,
        instrument_id: InstrumentId,
        channel: &str,
    ) -> anyhow::Result<()> {
        let client = self
            .inner
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket not connected"))?;
        let (raw_symbol, is_perp) = crate::common::parse::raw_symbol_from_instrument_id(&instrument_id);
        let product = if is_perp {
            BitgetProductType::UsdtFutures
        } else {
            BitgetProductType::Spot
        };
        let request = BitgetWsRequest::subscribe(&inst_type_str(product), channel, &raw_symbol);
        let text = serde_json::to_string(&request)?;
        client.send_text(text, None).await?;
        Ok(())
    }

    /// Disconnects the underlying socket.
    pub async fn close(&mut self) {
        if let Some(client) = self.inner.take() {
            client.disconnect().await;
        }
    }

    async fn read_loop(
        mut raw_rx: UnboundedReceiver<Message>,
        precisions: Arc<DashMap<InstrumentId, (u8, u8)>>,
        data_tx: UnboundedSender<Data>,
    ) {
        while let Some(msg) = raw_rx.recv().await {
            let text = match msg {
                Message::Text(t) => t.to_string(),
                Message::Binary(b) => match String::from_utf8(b.to_vec()) {
                    Ok(s) => s,
                    Err(_) => continue,
                },
                _ => continue,
            };

            // App-level pong keepalive response (CCXT `handle_pong`).
            if text == BITGET_WS_PONG || text == BITGET_WS_PING {
                continue;
            }

            let push: BitgetWsPush = match serde_json::from_str(&text) {
                Ok(p) => p,
                Err(e) => {
                    log::debug!("Bitget WS: undecodable message: {e} | {text}");
                    continue;
                }
            };

            if let Some(event) = &push.event {
                if event == "error" {
                    log::error!(
                        "Bitget WS error [{}]: {}",
                        push.code.unwrap_or_default(),
                        push.msg.unwrap_or_default()
                    );
                }
                continue; // subscribe/unsubscribe acks
            }

            let (Some(arg), Some(data)) = (push.arg, push.data) else {
                continue;
            };
            let product = product_from_inst_type(&arg.inst_type);
            let instrument_id = instrument_id_from_raw(&arg.inst_id, product);
            let Some(entry) = precisions.get(&instrument_id) else {
                log::debug!("Bitget WS: no precision for {instrument_id}, dropping push");
                continue;
            };
            let (price_precision, size_precision) = *entry;
            drop(entry);

            let ts_init = get_atomic_clock_realtime().get_time_ns();

            if arg.channel.starts_with("books") {
                let Some(first) = data.as_array().and_then(|a| a.first()) else {
                    continue;
                };
                let book: BitgetWsBook = match serde_json::from_value(first.clone()) {
                    Ok(b) => b,
                    Err(e) => {
                        log::warn!("Bitget WS: bad book payload: {e}");
                        continue;
                    }
                };
                match parse_book_snapshot(
                    &book,
                    instrument_id,
                    price_precision,
                    size_precision,
                    ts_init,
                ) {
                    Ok(deltas) => {
                        let _ = data_tx.send(Data::Deltas(OrderBookDeltas_API::new(deltas)));
                    }
                    Err(e) => log::warn!("Bitget WS: book parse failed: {e}"),
                }
            } else if arg.channel == BITGET_TRADE_CHANNEL {
                let trades: Vec<BitgetWsTrade> = match serde_json::from_value(data) {
                    Ok(t) => t,
                    Err(e) => {
                        log::warn!("Bitget WS: bad trade payload: {e}");
                        continue;
                    }
                };
                for trade in &trades {
                    match parse_ws_trade(
                        trade,
                        instrument_id,
                        price_precision,
                        size_precision,
                        ts_init,
                    ) {
                        Ok(tick) => {
                            let _ = data_tx.send(Data::Trade(tick));
                        }
                        Err(e) => log::warn!("Bitget WS: trade parse failed: {e}"),
                    }
                }
            }
        }
        log::debug!("Bitget WS read loop ended");
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_new_client_defaults() {
        let client = BitgetWebSocketClient::new(None);
        assert_eq!(client.url, BITGET_WS_PUBLIC_URL);
        assert!(!client.is_active());
    }

    #[rstest]
    fn test_subscribe_request_serialization() {
        let req = BitgetWsRequest::subscribe("SPOT", BITGET_BOOK_CHANNEL, "BTCUSDT");
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(
            json,
            r#"{"op":"subscribe","args":[{"instType":"SPOT","channel":"books15","instId":"BTCUSDT"}]}"#
        );
    }
}
