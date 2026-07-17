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

//! BingX public market-data WebSocket client.
//!
//! Ported first-hand from CCXT pro (`pro/bingx.py`). Key BingX quirks handled here:
//! - **Frames are gzip-compressed**: every inbound binary frame is `gunzip`'d before parsing
//!   (`options['ws']['gunzip'] = True`).
//! - **App-level keepalive**: the server sends the text `Ping` (after gunzip); the client must reply
//!   with the text `Pong` or the connection drops (~60s).
//! - **Subscribe**: `{ "id": <uuid>, "dataType": "<SYMBOL>@depth<N>" | "<SYMBOL>@trade" }`; swap
//!   streams additionally set `"reqType": "sub"`.
//!
//! Depth pushes are full limited-depth snapshots, so each is emitted as a `Clear` + `Add`* batch.

use std::{
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use dashmap::DashMap;
use flate2::read::GzDecoder;
use futures_util::{SinkExt, StreamExt};
use nautilus_common::messages::DataEvent;
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{data::Data, identifiers::InstrumentId};
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

use super::{
    messages::{BingXWsDepthData, BingXWsEnvelope, BingXWsRequest, BingXWsTradeData},
    parse::{parse_depth_snapshot, parse_trade_tick},
};
use crate::common::{
    consts::bingx_venue, enums::BingXProductType, parse::instrument_id_from_bingx_symbol,
};

/// Price/size precision for an instrument, cached so the read loop can quantize venue strings.
#[derive(Debug, Clone, Copy)]
struct Precisions {
    price: u8,
    size: u8,
}

/// A self-contained BingX public WebSocket client.
///
/// The read loop runs on a spawned task; outbound subscribe/pong messages are funneled through an
/// unbounded channel so callers can subscribe after connecting.
#[derive(Debug)]
pub struct BingXWebSocketClient {
    url: String,
    product: BingXProductType,
    data_sender: mpsc::UnboundedSender<DataEvent>,
    out_tx: Option<mpsc::UnboundedSender<Message>>,
    instruments: Arc<DashMap<InstrumentId, Precisions>>,
    subscriptions: Arc<DashMap<String, ()>>,
    is_connected: Arc<AtomicBool>,
    cancel: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl BingXWebSocketClient {
    /// Creates a new client for the given public stream `url` and `product`, forwarding parsed data
    /// through `data_sender`.
    #[must_use]
    pub fn new(
        url: String,
        product: BingXProductType,
        data_sender: mpsc::UnboundedSender<DataEvent>,
    ) -> Self {
        Self {
            url,
            product,
            data_sender,
            out_tx: None,
            instruments: Arc::new(DashMap::new()),
            subscriptions: Arc::new(DashMap::new()),
            is_connected: Arc::new(AtomicBool::new(false)),
            cancel: CancellationToken::new(),
            task: None,
        }
    }

    /// Returns `true` while the read loop holds an active connection.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Acquire)
    }

    /// Registers an instrument's precisions so the read loop can quantize prices/sizes.
    pub fn add_instrument(&self, instrument_id: InstrumentId, price_precision: u8, size_precision: u8) {
        self.instruments.insert(
            instrument_id,
            Precisions {
                price: price_precision,
                size: size_precision,
            },
        );
    }

    /// Connects to the stream and starts the background read loop.
    ///
    /// # Errors
    ///
    /// Returns an error if the initial connection fails.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let (ws_stream, _resp) = connect_async(&self.url).await?;
        let (mut writer, mut reader) = ws_stream.split();

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        self.out_tx = Some(out_tx.clone());
        self.is_connected.store(true, Ordering::Release);

        let data_sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let is_connected = self.is_connected.clone();
        let cancel = self.cancel.clone();

        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    outbound = out_rx.recv() => {
                        match outbound {
                            Some(msg) => {
                                if let Err(e) = writer.send(msg).await {
                                    log::warn!("BingX WS write error: {e}");
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                    frame = reader.next() => {
                        match frame {
                            Some(Ok(Message::Binary(bytes))) => {
                                if let Some(text) = gunzip_to_string(&bytes) {
                                    if handle_text(&text, &out_tx, &data_sender, &instruments) {
                                        continue;
                                    }
                                }
                            }
                            Some(Ok(Message::Text(text))) => {
                                handle_text(&text, &out_tx, &data_sender, &instruments);
                            }
                            Some(Ok(Message::Close(_))) => {
                                log::debug!("BingX WS closed by server");
                                break;
                            }
                            Some(Ok(_)) => {}
                            Some(Err(e)) => {
                                log::warn!("BingX WS error: {e}");
                                break;
                            }
                            None => break,
                        }
                    }
                    () = cancel.cancelled() => {
                        log::debug!("BingX WS task cancelled");
                        break;
                    }
                }
            }
            is_connected.store(false, Ordering::Release);
        });

        self.task = Some(task);
        Ok(())
    }

    /// Subscribes to the limited-depth order book for `instrument_id` (`@depth<levels>`).
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected.
    pub fn subscribe_book(&self, instrument_id: InstrumentId, levels: u32) -> anyhow::Result<()> {
        let symbol = instrument_id.symbol.as_str();
        let data_type = format!("{symbol}@depth{levels}");
        self.send_subscribe(data_type)
    }

    /// Subscribes to the trade stream for `instrument_id` (`@trade`).
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected.
    pub fn subscribe_trades(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        let symbol = instrument_id.symbol.as_str();
        let data_type = format!("{symbol}@trade");
        self.send_subscribe(data_type)
    }

    fn send_subscribe(&self, data_type: String) -> anyhow::Result<()> {
        let out_tx = self
            .out_tx
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("BingX WS not connected"))?;
        let req_type = if self.product.is_spot() {
            None
        } else {
            Some("sub".to_string())
        };
        let request = BingXWsRequest {
            id: uuid::Uuid::new_v4().to_string(),
            data_type: data_type.clone(),
            req_type,
        };
        let json = serde_json::to_string(&request)?;
        out_tx
            .send(Message::Text(json.into()))
            .map_err(|e| anyhow::anyhow!("failed to queue subscribe: {e}"))?;
        self.subscriptions.insert(data_type, ());
        Ok(())
    }

    /// Closes the connection and stops the read loop.
    pub fn close(&mut self) {
        self.cancel.cancel();
        self.is_connected.store(false, Ordering::Release);
        if let Some(out_tx) = &self.out_tx {
            let _ = out_tx.send(Message::Close(None));
        }
    }
}

/// Decompresses a gzip frame into a UTF-8 string. Returns `None` if it is neither valid gzip nor
/// valid UTF-8 (BingX occasionally sends short raw text such as `Ping`).
fn gunzip_to_string(bytes: &[u8]) -> Option<String> {
    let mut decoder = GzDecoder::new(bytes);
    let mut out = String::new();
    if decoder.read_to_string(&mut out).is_ok() && !out.is_empty() {
        return Some(out);
    }
    // Fall back to raw UTF-8 (uncompressed control frames).
    std::str::from_utf8(bytes).ok().map(ToString::to_string)
}

/// Handles a decoded text payload. Returns `true` if it was a control frame (`Ping`) so the caller
/// can `continue`. Data frames are parsed and forwarded via `data_sender`.
fn handle_text(
    text: &str,
    out_tx: &mpsc::UnboundedSender<Message>,
    data_sender: &mpsc::UnboundedSender<DataEvent>,
    instruments: &Arc<DashMap<InstrumentId, Precisions>>,
) -> bool {
    if text == "Ping" {
        let _ = out_tx.send(Message::Text("Pong".to_string().into()));
        return true;
    }
    if text.contains("\"ping\"") {
        // Spot JSON ping: {"ping":"<id>","time":"..."}. Echo back as a pong.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(text)
            && let Some(ping) = v.get("ping").and_then(|p| p.as_str())
        {
            let pong = serde_json::json!({"pong": ping, "time": v.get("time")});
            let _ = out_tx.send(Message::Text(pong.to_string().into()));
            return true;
        }
    }

    let envelope: BingXWsEnvelope = match serde_json::from_str(text) {
        Ok(e) => e,
        Err(_) => return false,
    };

    if envelope.data_type.is_empty() {
        // Subscription ack / status frame — nothing to forward.
        return false;
    }

    let symbol = envelope.data_type.split('@').next().unwrap_or_default();
    if symbol.is_empty() {
        return false;
    }
    let instrument_id = instrument_id_from_bingx_symbol(symbol);
    let Some(precisions) = instruments.get(&instrument_id).map(|p| *p) else {
        // Precision unknown (instrument not registered) — cannot quantize safely.
        log::debug!("BingX WS data for unregistered instrument {instrument_id}");
        return false;
    };

    // Ensure the venue matches (defensive; symbol split should already imply BINGX).
    debug_assert_eq!(instrument_id.venue, bingx_venue());

    let ts_init = UnixNanos::from(get_atomic_clock_realtime().get_time_ns());

    if envelope.data_type.contains("@depth") {
        match serde_json::from_value::<BingXWsDepthData>(envelope.data.clone()) {
            Ok(depth) => {
                let ts_event = envelope
                    .timestamp
                    .or(envelope.ts)
                    .map_or(ts_init, |ms| UnixNanos::from((ms.max(0) as u64) * 1_000_000));
                match parse_depth_snapshot(
                    &depth,
                    instrument_id,
                    precisions.price,
                    precisions.size,
                    ts_event,
                    ts_init,
                ) {
                    Ok(deltas) => {
                        if let Err(e) = data_sender.send(DataEvent::Data(Data::Deltas(deltas))) {
                            log::error!("Failed to emit BingX book deltas: {e}");
                        }
                    }
                    Err(e) => log::error!("Failed to parse BingX depth: {e}"),
                }
            }
            Err(e) => log::error!("Failed to deserialize BingX depth data: {e}"),
        }
    } else if envelope.data_type.contains("@trade") {
        match serde_json::from_value::<BingXWsTradeData>(envelope.data.clone()) {
            Ok(trade) => match parse_trade_tick(
                &trade,
                instrument_id,
                precisions.price,
                precisions.size,
                ts_init,
            ) {
                Ok(tick) => {
                    if let Err(e) = data_sender.send(DataEvent::Data(Data::Trade(tick))) {
                        log::error!("Failed to emit BingX trade: {e}");
                    }
                }
                Err(e) => log::error!("Failed to parse BingX trade: {e}"),
            },
            Err(e) => log::debug!("Non-trade BingX data on @trade channel: {e}"),
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;

    use super::*;

    fn gzip(text: &str) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(text.as_bytes()).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn gunzip_roundtrip() {
        let compressed = gzip("Ping");
        assert_eq!(gunzip_to_string(&compressed).as_deref(), Some("Ping"));
    }

    #[test]
    fn gunzip_falls_back_to_raw() {
        assert_eq!(gunzip_to_string(b"Ping").as_deref(), Some("Ping"));
    }

    #[test]
    fn ping_triggers_pong() {
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        let (data_tx, _data_rx) = mpsc::unbounded_channel::<DataEvent>();
        let instruments = Arc::new(DashMap::new());
        let handled = handle_text("Ping", &out_tx, &data_tx, &instruments);
        assert!(handled);
        match out_rx.try_recv().unwrap() {
            Message::Text(t) => assert_eq!(t.as_str(), "Pong"),
            other => panic!("expected Pong, got {other:?}"),
        }
    }

    #[test]
    fn depth_frame_emits_deltas() {
        let (out_tx, _out_rx) = mpsc::unbounded_channel::<Message>();
        let (data_tx, mut data_rx) = mpsc::unbounded_channel::<DataEvent>();
        let instruments = Arc::new(DashMap::new());
        let id = InstrumentId::from("BTC-USDT.BINGX");
        instruments.insert(id, Precisions { price: 2, size: 6 });

        let frame = r#"{"code":0,"dataType":"BTC-USDT@depth100","timestamp":1743241379958,"data":{"bids":[["83656.98","2.570805"]],"asks":[["84119.73","0.000011"]],"lastUpdateId":13565694850}}"#;
        assert!(!handle_text(frame, &out_tx, &data_tx, &instruments));

        match data_rx.try_recv().unwrap() {
            DataEvent::Data(Data::Deltas(_)) => {}
            other => panic!("expected book deltas, got {other:?}"),
        }
    }

    #[test]
    fn trade_frame_emits_trade() {
        let (out_tx, _out_rx) = mpsc::unbounded_channel::<Message>();
        let (data_tx, mut data_rx) = mpsc::unbounded_channel::<DataEvent>();
        let instruments = Arc::new(DashMap::new());
        let id = InstrumentId::from("BTC-USDT.BINGX");
        instruments.insert(id, Precisions { price: 2, size: 6 });

        let frame = r#"{"code":0,"dataType":"BTC-USDT@trade","data":{"E":1690214529432,"T":1690214529386,"e":"trade","m":true,"p":"29110.19","q":"0.1868","s":"BTC-USDT","t":"57903921"}}"#;
        assert!(!handle_text(frame, &out_tx, &data_tx, &instruments));

        match data_rx.try_recv().unwrap() {
            DataEvent::Data(Data::Trade(_)) => {}
            other => panic!("expected trade tick, got {other:?}"),
        }
    }
}
