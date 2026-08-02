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

//! LBank CONTRACT (USDT-perp) v3 WebSocket message models.
//!
//! Reverse-engineered from the LBank futures front-end (`cfdWsUrl` = `wss://uuws.../ws/v3`). The wire
//! format is a compact, key-renamed protocol (the front-end's `convertParams` maps readable keys to
//! single letters before sending):
//!
//! - Subscribe envelope: `{"x":<topic>,"a":{"i":"<instrumentId>"},"z":<type>,"y":<sub_id>}`
//!   - `x` topic (numeric `KK` enum): `OrderBook=3`, `Deal=4`.
//!   - `z` type: `Sub=1`, `UnSub=0`.
//!   - Deal `i` is the bare symbol (`BTCUSDT`); OrderBook `i` is a COMPOSITE
//!     `"{symbol}_{decimal}_{limit}"` (a bare symbol is rejected with a `z=21` ack and no data).
//! - Keepalive: the client MUST send `{"action":"ping","ping":"<ms>"}` every ~5s or the server
//!   closes the socket (`1000 OK`). The server's own `{"w":ms,"x":0,"z":11}` heartbeats are ignored.
//! - Depth push (`x=3`): bids/asks are TOP-LEVEL arrays of `[price, volume]` string pairs —
//!   `{"b":[["63227","20.38"],..],"s":[["63229","1.1"],..],"w":ms,"x":3,..}` (`b`=bid, `s`=ask).
//! - Trade push (`x=4`): `{"d":{"a":sym,"b":vol,"c":price,"d":dir,"e":ts_sec,"f":trade_id},"x":4,..}`
//!   (`d`=direction `"0"`=buy/`"1"`=sell). On subscribe the first frame's `d` is an ARRAY snapshot.

use serde::{Deserialize, Serialize};

use crate::common::consts::{
    CONTRACT_WS_TOPIC_DEAL, CONTRACT_WS_TOPIC_ORDERBOOK, CONTRACT_WS_TYPE_SUB,
};

/// Outgoing subscribe/unsubscribe envelope (already in the compact wire shape).
#[derive(Clone, Debug, Serialize)]
pub struct ContractWsSubscribe {
    /// Topic (`x`): 3 = OrderBook, 4 = Deal.
    pub x: u8,
    /// Params (`a`), containing the instrument id under `i`.
    pub a: ContractWsParam,
    /// Type (`z`): 1 = Sub, 0 = UnSub.
    pub z: u8,
    /// Client-assigned subscription id (`y`), echoed on the ack.
    pub y: u64,
}

/// The `a` param object: `{"i":"<instrumentId>"}`.
#[derive(Clone, Debug, Serialize)]
pub struct ContractWsParam {
    /// Instrument id (bare `BTCUSDT` for Deal; `BTCUSDT_{decimal}_{limit}` for OrderBook).
    pub i: String,
}

impl ContractWsSubscribe {
    /// Builds an OrderBook (depth) subscribe for `symbol`. `price_tick` is the price-group STEP as a
    /// string (the instrument's native tick, e.g. `"0.1"`, gives the finest book); `limit` is the
    /// level count (LBank fixes this at [`CONTRACT_WS_DEPTH_LIMIT`] = 25). The composite id is
    /// `"{symbol}_{price_tick}_{limit}"`.
    #[must_use]
    pub fn order_book(symbol: &str, price_tick: &str, limit: u32, sub_id: u64) -> Self {
        Self {
            x: CONTRACT_WS_TOPIC_ORDERBOOK,
            a: ContractWsParam {
                i: format!("{symbol}_{price_tick}_{limit}"),
            },
            z: CONTRACT_WS_TYPE_SUB,
            y: sub_id,
        }
    }

    /// Builds a Deal (trades) subscribe for `symbol`.
    #[must_use]
    pub fn deal(symbol: &str, sub_id: u64) -> Self {
        Self {
            x: CONTRACT_WS_TOPIC_DEAL,
            a: ContractWsParam {
                i: symbol.to_string(),
            },
            z: CONTRACT_WS_TYPE_SUB,
            y: sub_id,
        }
    }
}

/// Outgoing keepalive ping (`{"action":"ping","ping":"<ms>"}`).
#[derive(Clone, Debug, Serialize)]
pub struct ContractWsPing {
    /// Always `ping`.
    pub action: String,
    /// Current time in milliseconds, as a string.
    pub ping: String,
}

/// A single trade payload (`d` of a Deal push, or one element of the subscribe snapshot array).
#[derive(Clone, Debug, Deserialize)]
pub struct ContractWsTrade {
    /// `a` — instrument id (e.g. `BTCUSDT`).
    #[serde(default)]
    pub a: Option<String>,
    /// `b` — trade volume (base), decimal string.
    pub b: String,
    /// `c` — trade price, decimal string.
    pub c: String,
    /// `d` — direction: `"0"` = buy(taker), `"1"` = sell(taker).
    #[serde(default)]
    pub d: Option<String>,
    /// `e` — trade time in SECONDS (decimal string).
    #[serde(default)]
    pub e: Option<String>,
    /// `f` — trade id.
    #[serde(default)]
    pub f: Option<String>,
}

/// The `d` field of a Deal push: either a single trade or a snapshot array of trades.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum ContractDealData {
    /// Snapshot: an array of recent trades (sent once right after subscribe).
    Many(Vec<ContractWsTrade>),
    /// A single incremental trade.
    One(ContractWsTrade),
}

/// A raw inbound v3 frame. Only the fields we need are captured; topic (`x`) discriminates.
#[derive(Clone, Debug, Deserialize)]
pub struct ContractWsFrame {
    /// Topic id (`x`): 3 = OrderBook, 4 = Deal, 0 = server heartbeat.
    #[serde(default)]
    pub x: Option<i64>,
    /// Server timestamp in milliseconds (`w`), present on data + heartbeat frames.
    #[serde(default)]
    pub w: Option<i64>,
    /// Response/push type (`z`): 4 = push, 11 = heartbeat, 13/14/21 = subscribe acks.
    #[serde(default)]
    pub z: Option<i64>,
    /// Bid levels (`b`) on a depth push — top-level array of `[price, volume]`.
    #[serde(default)]
    pub b: Option<Vec<[String; 2]>>,
    /// Ask levels (`s`) on a depth push — top-level array of `[price, volume]`.
    #[serde(default)]
    pub s: Option<Vec<[String; 2]>>,
    /// Trade payload (`d`) on a Deal push.
    #[serde(default)]
    pub d: Option<ContractDealData>,
    /// Subscription id (`y`) echoed on every data/ack frame — the ONLY way to attribute a depth push
    /// to an instrument (depth frames carry no symbol). Returned as a string (we send it as a number).
    #[serde(default)]
    pub y: Option<String>,
    /// Client keepalive echo — present only on our own ping shape (`{"action":"ping"}`); the server
    /// does not send this, but keep it so an unexpected app ping is recognisable.
    #[serde(default)]
    pub action: Option<String>,
    /// Ping token, if an app-level ping is ever received.
    #[serde(default)]
    pub ping: Option<String>,
}

impl ContractWsFrame {
    /// `true` when this is a depth (OrderBook) push carrying bid/ask levels.
    #[must_use]
    pub fn is_depth(&self) -> bool {
        self.x == Some(i64::from(CONTRACT_WS_TOPIC_ORDERBOOK)) && (self.b.is_some() || self.s.is_some())
    }

    /// `true` when this is a Deal push carrying trade data.
    #[must_use]
    pub fn is_trade(&self) -> bool {
        self.x == Some(i64::from(CONTRACT_WS_TOPIC_DEAL)) && self.d.is_some()
    }

    /// Parses the subscription id (`y`) to a `u64`, used to attribute the frame to an instrument.
    #[must_use]
    pub fn sub_id(&self) -> Option<u64> {
        self.y.as_deref().and_then(|s| s.trim().parse::<u64>().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_book_subscribe_is_composite() {
        let sub = ContractWsSubscribe::order_book("BTCUSDT", "0.1", 25, 7);
        let json = serde_json::to_string(&sub).unwrap();
        assert!(json.contains(r#""x":3"#));
        assert!(json.contains(r#""i":"BTCUSDT_0.1_25""#));
        assert!(json.contains(r#""z":1"#));
        assert!(json.contains(r#""y":7"#));
    }

    #[test]
    fn deal_subscribe_is_bare_symbol() {
        let sub = ContractWsSubscribe::deal("BTCUSDT", 2);
        let json = serde_json::to_string(&sub).unwrap();
        assert!(json.contains(r#""x":4"#));
        assert!(json.contains(r#""i":"BTCUSDT""#));
    }

    #[test]
    fn parse_depth_frame() {
        let raw = r#"{"b":[["63227","20.38"],["63226","6.77"]],"s":[["63229","1.1"]],"w":1785662431000,"x":3,"z":4}"#;
        let f: ContractWsFrame = serde_json::from_str(raw).unwrap();
        assert!(f.is_depth());
        assert_eq!(f.b.as_ref().unwrap().len(), 2);
        assert_eq!(f.s.as_ref().unwrap().len(), 1);
        assert_eq!(f.b.as_ref().unwrap()[0], ["63227".to_string(), "20.38".to_string()]);
    }

    #[test]
    fn parse_trade_frame_single() {
        let raw = r#"{"d":{"a":"BTCUSDT","b":"0.0012","c":"63435.1","d":"0","e":"1785659150","f":"1007931922450694"},"w":1785659150935,"x":4,"y":"2","z":4}"#;
        let f: ContractWsFrame = serde_json::from_str(raw).unwrap();
        assert!(f.is_trade());
        match f.d.unwrap() {
            ContractDealData::One(t) => {
                assert_eq!(t.c, "63435.1");
                assert_eq!(t.d.as_deref(), Some("0"));
            }
            ContractDealData::Many(_) => panic!("expected single"),
        }
    }

    #[test]
    fn parse_trade_frame_snapshot_array() {
        let raw = r#"{"d":[{"a":"BTCUSDT","b":"0.0028","c":"63435.2","d":"0","e":"1","f":"1"},{"a":"BTCUSDT","b":"0.0034","c":"63435.1","d":"1","e":"1","f":"2"}]}"#;
        let f: ContractWsFrame = serde_json::from_str(raw).unwrap();
        match f.d.unwrap() {
            ContractDealData::Many(v) => assert_eq!(v.len(), 2),
            ContractDealData::One(_) => panic!("expected array"),
        }
    }

    #[test]
    fn heartbeat_frame_is_neither() {
        let f: ContractWsFrame = serde_json::from_str(r#"{"w":1785662436070,"x":0,"z":11}"#).unwrap();
        assert!(!f.is_depth());
        assert!(!f.is_trade());
    }
}
