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

//! Serde models for BingX WebSocket messages.
//!
//! Shapes ported first-hand from CCXT pro (`pro/bingx.py`: `watch_order_book`/`handle_order_book`,
//! `watch_trades`/`handle_trades`). Every push shares the `{code, dataType, data, timestamp}`
//! envelope; `dataType` is `<SYMBOL>@depth<N>` or `<SYMBOL>@trade`.

use serde::{Deserialize, Serialize};

/// Outbound subscribe/unsubscribe request. Spot streams omit `req_type`; swap streams set it to
/// `"sub"`/`"unsub"` (CCXT `watch_*`).
#[derive(Debug, Clone, Serialize)]
pub struct BingXWsRequest {
    pub id: String,
    #[serde(rename = "dataType")]
    pub data_type: String,
    #[serde(rename = "reqType", skip_serializing_if = "Option::is_none")]
    pub req_type: Option<String>,
}

/// Generic inbound envelope. `data` is left raw so the dispatcher can decode by `data_type`.
#[derive(Debug, Clone, Deserialize)]
pub struct BingXWsEnvelope {
    #[serde(default)]
    pub code: i64,
    #[serde(default, rename = "dataType")]
    pub data_type: String,
    #[serde(default)]
    pub data: serde_json::Value,
    #[serde(default)]
    pub timestamp: Option<i64>,
    #[serde(default)]
    pub ts: Option<i64>,
}

/// Depth push `data` (`handle_order_book`, spot/linear form with `[price, size]` array levels).
#[derive(Debug, Clone, Deserialize)]
pub struct BingXWsDepthData {
    #[serde(default)]
    pub bids: Vec<[String; 2]>,
    #[serde(default)]
    pub asks: Vec<[String; 2]>,
    #[serde(default, rename = "lastUpdateId")]
    pub last_update_id: Option<i64>,
}

/// Trade push `data` (`handle_trades`, spot single-trade form).
#[derive(Debug, Clone, Deserialize)]
pub struct BingXWsTradeData {
    /// Price.
    pub p: String,
    /// Quantity (base).
    pub q: String,
    /// Trade id (BingX sends a string for spot).
    #[serde(default, deserialize_with = "de_string_from_any")]
    pub t: String,
    /// `true` when the buyer was the maker (aggressor = seller).
    #[serde(default)]
    pub m: bool,
    /// Trade time (ms).
    #[serde(rename = "T", default)]
    pub trade_time: i64,
    /// Symbol.
    #[serde(default)]
    pub s: String,
}

/// Deserializes a JSON string OR number into a `String` (BingX trade ids vary by product).
fn de_string_from_any<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    })
}
