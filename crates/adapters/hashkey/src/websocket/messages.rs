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

//! Serde models for HashKey WebSocket push messages (first-hand from CCXT `pro/hashkey.py`).

use nautilus_model::data::Data;
use serde::{Deserialize, Serialize};

/// A public-stream push envelope: `{symbol, topic, data:[...], f, sendTime}`.
///
/// `topic` is `depth` for order books and `trade` for trades. The `data` array holds one entry for
/// depth (a full snapshot) or many for trades.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HashKeyWsPush {
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub data: Vec<serde_json::Value>,
}

/// A depth snapshot entry (`{e, s, t, v, b:[[px,qty]], a:[[px,qty]], o}`).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HashKeyWsDepth {
    pub t: i64,
    #[serde(default)]
    pub b: Vec<[String; 2]>,
    #[serde(default)]
    pub a: Vec<[String; 2]>,
}

/// A trade entry (`{v: id, t: ts, p, q, m: isBuyerMaker}`).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HashKeyWsTrade {
    #[serde(default)]
    pub v: String,
    pub t: i64,
    pub p: String,
    pub q: String,
    /// `isBuyerMaker` — when true the aggressor was the seller.
    #[serde(default)]
    pub m: bool,
}

/// A `{"ping": <ts>}` frame sent by the HashKey server (client replies with `{"pong": <ts>}`).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HashKeyWsPing {
    pub ping: i64,
}

/// The parsed, adapter-level outbound message forwarded to the data client's stream.
#[derive(Clone, Debug)]
pub enum HashKeyWsMessage {
    /// One or more Nautilus data items (trades, book deltas) ready to emit.
    Data(Vec<Data>),
}
