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

//! Gate WebSocket message models (mirrors CCXT pro `gate.py` push shapes).

use nautilus_model::data::{OrderBookDeltas, TradeTick};
use serde::{Deserialize, Serialize};

use crate::http::models::{SpotBookLevel, SpotTrade};

/// Outgoing subscribe/unsubscribe request (per CCXT `subscribe_public`).
#[derive(Clone, Debug, Serialize)]
pub struct WsRequest {
    /// Client-generated request id.
    pub id: u64,
    /// Request time in seconds.
    pub time: i64,
    /// Channel name, e.g. `spot.order_book` or `spot.trades`.
    pub channel: String,
    /// `subscribe` or `unsubscribe`.
    pub event: String,
    /// Channel-specific payload (e.g. `["BTC_USDT","20","100ms"]`).
    pub payload: Vec<String>,
}

/// Generic inbound envelope shared by all Gate channels.
#[derive(Clone, Debug, Deserialize)]
pub struct WsEnvelope {
    /// Server time (seconds).
    #[serde(default)]
    pub time: Option<i64>,
    /// Channel name (e.g. `spot.order_book`, `spot.trades`, `spot.pong`).
    pub channel: String,
    /// Event type (`subscribe`, `unsubscribe`, `update`, `all`).
    #[serde(default)]
    pub event: Option<String>,
    /// Error object when the request failed.
    #[serde(default)]
    pub error: Option<WsError>,
    /// Channel-specific result payload.
    #[serde(default)]
    pub result: serde_json::Value,
}

/// Error object embedded in an [`WsEnvelope`].
#[derive(Clone, Debug, Deserialize)]
pub struct WsError {
    /// Numeric error code.
    #[serde(default)]
    pub code: Option<i64>,
    /// Human-readable message.
    #[serde(default)]
    pub message: Option<String>,
}

/// `spot.order_book` full-snapshot push result.
#[derive(Clone, Debug, Deserialize)]
pub struct WsSpotOrderBook {
    /// Snapshot timestamp (ms).
    #[serde(default)]
    pub t: Option<i64>,
    /// Market id (`BTC_USDT`).
    pub s: String,
    /// Bid levels (`[price, amount]`).
    #[serde(default)]
    pub bids: Vec<SpotBookLevel>,
    /// Ask levels (`[price, amount]`).
    #[serde(default)]
    pub asks: Vec<SpotBookLevel>,
}

/// A parsed Nautilus message emitted by the Gate WebSocket stream.
#[derive(Clone, Debug)]
pub enum GateWsMessage {
    /// Order book snapshot deltas.
    Deltas(OrderBookDeltas),
    /// A single trade tick.
    Trade(TradeTick),
    /// The connection was re-established (subscriptions replayed by the transport).
    Reconnected,
}

/// Alias re-export for the trade push payload (identical shape to REST trades).
pub type WsSpotTrade = SpotTrade;
