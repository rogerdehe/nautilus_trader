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

//! Serde models for Gate APIv4 REST responses (field names/units mirror CCXT `gate.py`).

use serde::{Deserialize, Serialize};

/// A spot market from `GET /spot/currency_pairs` (see CCXT `fetch_spot_markets`).
#[derive(Clone, Debug, Deserialize)]
pub struct SpotCurrencyPair {
    /// Market id, e.g. `BTC_USDT`.
    pub id: String,
    /// Base currency code, e.g. `BTC`.
    pub base: String,
    /// Quote currency code, e.g. `USDT`.
    pub quote: String,
    /// Number of decimal places for the amount (base size).
    #[serde(default)]
    pub amount_precision: u8,
    /// Number of decimal places for the price.
    #[serde(default)]
    pub precision: u8,
    /// Minimum order size in base currency (string decimal).
    #[serde(default)]
    pub min_base_amount: Option<String>,
    /// Minimum order cost in quote currency (string decimal).
    #[serde(default)]
    pub min_quote_amount: Option<String>,
    /// Maximum order cost in quote currency (string decimal).
    #[serde(default)]
    pub max_quote_amount: Option<String>,
    /// Taker fee as a percentage (e.g. `"0.2"` = 0.2%).
    #[serde(default)]
    pub fee: Option<String>,
    /// Maker fee rate as a percentage (falls back to `fee`).
    #[serde(default)]
    pub maker_fee_rate: Option<String>,
    /// Trade status, `tradable` when active.
    #[serde(default)]
    pub trade_status: Option<String>,
}

/// A `[price, amount]` order book level.
#[derive(Clone, Debug, Deserialize)]
pub struct SpotBookLevel(pub String, pub String);

/// Response of `GET /spot/order_book`.
#[derive(Clone, Debug, Deserialize)]
pub struct SpotOrderBook {
    /// Order book update id.
    #[serde(default)]
    pub id: Option<i64>,
    /// Server current timestamp (ms).
    #[serde(default)]
    pub current: Option<i64>,
    /// Order book last update timestamp (ms).
    #[serde(default)]
    pub update: Option<i64>,
    /// Ask levels (ascending price).
    #[serde(default)]
    pub asks: Vec<SpotBookLevel>,
    /// Bid levels (descending price).
    #[serde(default)]
    pub bids: Vec<SpotBookLevel>,
}

/// A trade from `GET /spot/trades` (also the WS `spot.trades` payload).
#[derive(Clone, Debug, Deserialize)]
pub struct SpotTrade {
    /// Trade id.
    pub id: String,
    /// Trade time in seconds (string).
    #[serde(default)]
    pub create_time: Option<String>,
    /// Trade time in milliseconds (string with fractional part).
    #[serde(default)]
    pub create_time_ms: Option<String>,
    /// Taker side (`buy` or `sell`).
    pub side: String,
    /// Market id (present on the WS push and cross-pair REST queries).
    #[serde(default)]
    pub currency_pair: Option<String>,
    /// Trade amount (base size).
    pub amount: String,
    /// Trade price.
    pub price: String,
}

/// Request body for `POST /spot/orders` (see CCXT `create_order`).
#[derive(Clone, Debug, Serialize)]
pub struct SpotOrderRequest {
    /// Market id (`BTC_USDT`).
    pub currency_pair: String,
    /// `buy` or `sell`.
    pub side: String,
    /// `limit` or `market`.
    #[serde(rename = "type")]
    pub order_type: String,
    /// Order size in base currency (string decimal).
    pub amount: String,
    /// Limit price (omitted for market orders).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    /// Time in force (`gtc`, `ioc`, `poc`, `fok`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<String>,
    /// Client-supplied order text/id (must start with `t-`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Response of `POST /spot/orders` / `DELETE /spot/orders/{id}` (see CCXT `parse_order`).
#[derive(Clone, Debug, Deserialize)]
pub struct SpotOrder {
    /// Venue order id.
    pub id: String,
    /// Client-supplied text/id.
    #[serde(default)]
    pub text: Option<String>,
    /// Market id.
    #[serde(default)]
    pub currency_pair: Option<String>,
    /// Order status (`open`, `closed`, `cancelled`).
    #[serde(default)]
    pub status: Option<String>,
    /// Order type (`limit`, `market`).
    #[serde(rename = "type", default)]
    pub order_type: Option<String>,
    /// Order side (`buy`, `sell`).
    #[serde(default)]
    pub side: Option<String>,
    /// Original amount (base).
    #[serde(default)]
    pub amount: Option<String>,
    /// Order price.
    #[serde(default)]
    pub price: Option<String>,
    /// Time in force.
    #[serde(default)]
    pub time_in_force: Option<String>,
    /// Remaining unfilled amount.
    #[serde(default)]
    pub left: Option<String>,
    /// Cumulative filled value (quote).
    #[serde(default)]
    pub filled_total: Option<String>,
    /// Average fill price.
    #[serde(default)]
    pub fill_price: Option<String>,
    /// Terminal reason (`filled`, `cancelled`, ...).
    #[serde(default)]
    pub finish_as: Option<String>,
}

/// A spot balance row from `GET /spot/accounts`.
#[derive(Clone, Debug, Deserialize)]
pub struct SpotAccount {
    /// Currency code (`USDT`).
    pub currency: String,
    /// Free/available balance.
    #[serde(default)]
    pub available: Option<String>,
    /// Locked balance.
    #[serde(default)]
    pub locked: Option<String>,
}
