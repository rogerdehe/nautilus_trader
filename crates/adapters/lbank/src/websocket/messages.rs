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

//! LBank spot WebSocket message models (mirrors CCXT pro `lbank.py` push shapes).

use nautilus_model::data::{OrderBookDeltas, TradeTick};
use serde::{Deserialize, Serialize};

use crate::http::models::{FlexStr, LBankLevel};

/// Outgoing subscribe request, e.g.
/// `{"action":"subscribe","subscribe":"depth","depth":100,"pair":"btc_usdt"}`.
///
/// Per CCXT `watch_order_book` the `depth` value is sent as an integer (the API doc shows a string;
/// CCXT is first-hand so we follow it — flagged for a live smoke test).
#[derive(Clone, Debug, Serialize)]
pub struct WsSubscribeRequest {
    /// `subscribe` or `unsubscribe`.
    pub action: String,
    /// Channel, e.g. `depth` or `trade`.
    pub subscribe: String,
    /// Depth level count (only for the `depth` channel).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    /// LBank pair id (`btc_usdt`).
    pub pair: String,
}

/// Outgoing pong reply to the mandatory app-level ping (`{"action":"pong","pong":"<uuid>"}`).
#[derive(Clone, Debug, Serialize)]
pub struct WsPong {
    /// Always `pong`.
    pub action: String,
    /// The `ping` uuid echoed back.
    pub pong: String,
}

/// A trade push payload: `{"volume":..,"price":..,"direction":"sell","TS":".."}`.
#[derive(Clone, Debug, Deserialize)]
pub struct WsTrade {
    /// Trade quantity (base).
    pub volume: FlexStr,
    /// Trade price.
    pub price: FlexStr,
    /// Aggressor direction: `buy`/`sell` and suffixed variants.
    #[serde(default)]
    pub direction: Option<String>,
    /// ISO-8601 trade timestamp (no timezone; treated as UTC).
    #[serde(rename = "TS", default)]
    pub ts: Option<String>,
}

/// Generic inbound envelope shared by all LBank spot channels. Detects the app ping, depth and
/// trade pushes, and error frames (`status == "error"`).
#[derive(Clone, Debug, Deserialize)]
pub struct WsEnvelope {
    /// Control action (`ping`), when present.
    #[serde(default)]
    pub action: Option<String>,
    /// The ping uuid (present on `{"action":"ping","ping":"<uuid>"}`).
    #[serde(default)]
    pub ping: Option<String>,
    /// Push type (`depth`, `trade`, `kbar`, `tick`).
    #[serde(rename = "type", default)]
    pub push_type: Option<String>,
    /// Market id (`btc_usdt`).
    #[serde(default)]
    pub pair: Option<String>,
    /// ISO-8601 push timestamp (no timezone; treated as UTC).
    #[serde(rename = "TS", default)]
    pub ts: Option<String>,
    /// Status field; `error` marks a rejected request.
    #[serde(default)]
    pub status: Option<String>,
    /// Error message (present on `status == "error"`).
    #[serde(default)]
    pub message: Option<String>,
    /// Depth snapshot nested under `depth` (the subscribe-push shape).
    #[serde(default)]
    pub depth: Option<WsDepth>,
    /// Ask levels when the depth arrives at top level (the "request" push shape).
    #[serde(default)]
    pub asks: Vec<LBankLevel>,
    /// Bid levels when the depth arrives at top level.
    #[serde(default)]
    pub bids: Vec<LBankLevel>,
    /// Trade payload nested under `trade`.
    #[serde(default)]
    pub trade: Option<WsTrade>,
}

/// The nested `depth` object of a depth push.
#[derive(Clone, Debug, Deserialize)]
pub struct WsDepth {
    /// Ask levels (ascending price).
    #[serde(default)]
    pub asks: Vec<LBankLevel>,
    /// Bid levels (descending price).
    #[serde(default)]
    pub bids: Vec<LBankLevel>,
}

/// A parsed message emitted by the LBank WebSocket stream.
#[derive(Clone, Debug)]
pub enum LbankWsMessage {
    /// A mandatory app-level ping carrying the uuid to echo back as a pong.
    Ping(String),
    /// Order book snapshot deltas.
    Deltas(OrderBookDeltas),
    /// A single trade tick.
    Trade(TradeTick),
}
