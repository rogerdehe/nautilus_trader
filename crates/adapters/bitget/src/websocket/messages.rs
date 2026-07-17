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

//! Serde models for Bitget v2 WebSocket messages (ported from CCXT `pro/bitget.py`).

use serde::{Deserialize, Serialize};

/// Depth channel used for order book subscriptions (snapshot, top 15 levels).
pub const BITGET_BOOK_CHANNEL: &str = "books15";
/// Trade channel used for trade subscriptions.
pub const BITGET_TRADE_CHANNEL: &str = "trade";

/// A single subscription argument (`{instType, channel, instId}`).
#[derive(Clone, Debug, Serialize)]
pub struct BitgetSubscriptionArg {
    #[serde(rename = "instType")]
    pub inst_type: String,
    pub channel: String,
    #[serde(rename = "instId")]
    pub inst_id: String,
}

/// A subscribe / unsubscribe request (`{op, args}`).
#[derive(Clone, Debug, Serialize)]
pub struct BitgetWsRequest {
    pub op: String,
    pub args: Vec<BitgetSubscriptionArg>,
}

impl BitgetWsRequest {
    /// Builds a `subscribe` request for a single channel/instrument.
    #[must_use]
    pub fn subscribe(inst_type: &str, channel: &str, inst_id: &str) -> Self {
        Self {
            op: "subscribe".to_string(),
            args: vec![BitgetSubscriptionArg {
                inst_type: inst_type.to_string(),
                channel: channel.to_string(),
                inst_id: inst_id.to_string(),
            }],
        }
    }

    /// Builds an `unsubscribe` request for a single channel/instrument.
    #[must_use]
    pub fn unsubscribe(inst_type: &str, channel: &str, inst_id: &str) -> Self {
        Self {
            op: "unsubscribe".to_string(),
            args: vec![BitgetSubscriptionArg {
                inst_type: inst_type.to_string(),
                channel: channel.to_string(),
                inst_id: inst_id.to_string(),
            }],
        }
    }
}

/// The `arg` object present on push messages and subscribe acknowledgements.
#[derive(Clone, Debug, Deserialize)]
pub struct BitgetWsArg {
    #[serde(rename = "instType", default)]
    pub inst_type: String,
    #[serde(default)]
    pub channel: String,
    #[serde(rename = "instId", default)]
    pub inst_id: String,
}

/// A generic incoming Bitget WebSocket push message.
#[derive(Clone, Debug, Deserialize)]
pub struct BitgetWsPush {
    /// Present on subscribe/unsubscribe acknowledgements and errors (`subscribe`/`error`).
    #[serde(default)]
    pub event: Option<String>,
    /// Present on data pushes (`snapshot` / `update`).
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub arg: Option<BitgetWsArg>,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    /// Error code (on `event == "error"`).
    #[serde(default)]
    pub code: Option<i64>,
    #[serde(default)]
    pub msg: Option<String>,
}

/// Order book snapshot payload (`data[0]` on a depth push).
#[derive(Clone, Debug, Deserialize)]
pub struct BitgetWsBook {
    #[serde(default)]
    pub asks: Vec<[String; 2]>,
    #[serde(default)]
    pub bids: Vec<[String; 2]>,
    /// Timestamp (millisecond epoch as string).
    pub ts: String,
}

/// Public trade payload (element of `data` on a trade push).
#[derive(Clone, Debug, Deserialize)]
pub struct BitgetWsTrade {
    pub ts: String,
    pub price: String,
    pub size: String,
    pub side: String,
    #[serde(rename = "tradeId")]
    pub trade_id: String,
}
