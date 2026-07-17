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

//! Serde models for KuCoin WebSocket frames (first-hand from `ccxt/python/ccxt/pro/kucoin.py`).

use serde::{Deserialize, Serialize};

/// An order book level `[price, size]`.
pub type KuCoinWsLevel = [String; 2];

/// A generic inbound KuCoin WebSocket frame.
///
/// `type` routes control frames (`welcome`/`ack`/`pong`/`error`) vs data (`message`).
#[derive(Clone, Debug, Deserialize)]
pub struct KuCoinWsMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub code: Option<serde_json::Value>,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

/// `/spotMarket/level2Depth5|50` push payload — a FULL top-N snapshot each message.
#[derive(Clone, Debug, Deserialize)]
pub struct KuCoinWsDepth {
    #[serde(default)]
    pub asks: Vec<KuCoinWsLevel>,
    #[serde(default)]
    pub bids: Vec<KuCoinWsLevel>,
    /// Server time in milliseconds.
    #[serde(default)]
    pub timestamp: i64,
}

/// `/market/match` push payload (subject `trade.l3match`).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KuCoinWsTrade {
    pub symbol: String,
    pub side: String,
    pub size: String,
    pub price: String,
    /// Match time in NANOSECONDS (string).
    pub time: String,
    pub trade_id: String,
    #[serde(default)]
    pub sequence: Option<String>,
}

/// Outbound subscribe/unsubscribe request.
#[derive(Clone, Debug, Serialize)]
pub struct KuCoinWsSubscribe {
    pub id: String,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub topic: String,
    #[serde(rename = "privateChannel")]
    pub private_channel: bool,
    pub response: bool,
}

impl KuCoinWsSubscribe {
    /// Builds a public `subscribe` request for `topic`.
    #[must_use]
    pub fn subscribe(id: String, topic: String) -> Self {
        Self {
            id,
            msg_type: "subscribe".to_string(),
            topic,
            private_channel: false,
            response: true,
        }
    }
}

/// Outbound application-level ping (`{"id":..,"type":"ping"}`).
#[derive(Clone, Debug, Serialize)]
pub struct KuCoinWsPing {
    pub id: String,
    #[serde(rename = "type")]
    pub msg_type: String,
}

impl KuCoinWsPing {
    /// Builds a ping frame with the given id.
    #[must_use]
    pub fn new(id: String) -> Self {
        Self {
            id,
            msg_type: "ping".to_string(),
        }
    }
}

/// Extracts the `BASE-QUOTE` symbol from a KuCoin WS topic like `/market/match:BTC-USDT`.
#[must_use]
pub fn symbol_from_topic(topic: &str) -> Option<&str> {
    topic.split(':').nth(1)
}
