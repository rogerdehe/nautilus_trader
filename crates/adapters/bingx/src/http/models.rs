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

//! Serde models for BingX REST responses.
//!
//! Shapes ported first-hand from CCXT (`bingx.py`): `spot/v1/common/symbols`, `spot/v1/market/depth`,
//! `spot/v1/market/trades`, `spot/v1/trade/order`, `spot/v1/account/balance`. Every response is
//! wrapped in `{code, msg, data}`; BingX renders numeric fields as JSON numbers, so numeric fields
//! that feed Nautilus precision (tick/step sizes, prices) are captured as `f64` and normalized
//! through `rust_decimal` at parse time (serde_json would otherwise emit e.g. `1e-6`).

use serde::Deserialize;

/// Generic BingX response envelope. `data` is left as raw JSON so each endpoint can deserialize
/// its own payload after the `code` check.
#[derive(Debug, Clone, Deserialize)]
pub struct BingXResponse<T> {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub msg: String,
    pub data: Option<T>,
}

/// `data` payload of `spot/v1/common/symbols`.
#[derive(Debug, Clone, Deserialize)]
pub struct BingXSpotSymbols {
    #[serde(default)]
    pub symbols: Vec<BingXSpotSymbol>,
}

/// A single spot market (`parse_market` in CCXT).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BingXSpotSymbol {
    /// Market id, e.g. `BTC-USDT`.
    pub symbol: String,
    /// Price increment (quote precision).
    #[serde(default)]
    pub tick_size: f64,
    /// Quantity increment (base precision).
    #[serde(default)]
    pub step_size: f64,
    /// Minimum notional (quote) per order.
    #[serde(default)]
    pub min_notional: f64,
    /// Maximum notional (quote) per order.
    #[serde(default)]
    pub max_notional: f64,
    /// `1` when the market is online/tradable.
    #[serde(default)]
    pub status: i64,
    /// Whether the market can be sold via API.
    #[serde(default)]
    pub api_state_sell: bool,
    /// Whether the market can be bought via API.
    #[serde(default)]
    pub api_state_buy: bool,
}

/// `data` payload of `spot/v1/market/depth`.
#[derive(Debug, Clone, Deserialize)]
pub struct BingXSpotDepth {
    #[serde(default)]
    pub bids: Vec<[String; 2]>,
    #[serde(default)]
    pub asks: Vec<[String; 2]>,
    #[serde(default)]
    pub ts: Option<i64>,
    #[serde(default, rename = "lastUpdateId")]
    pub last_update_id: Option<i64>,
}

/// One element of the `spot/v1/market/trades` `data` array (`parse_trade`, spot fetchTrades).
#[derive(Debug, Clone, Deserialize)]
pub struct BingXSpotTrade {
    pub id: i64,
    /// Trade price.
    pub price: f64,
    /// Trade quantity (base).
    pub qty: f64,
    /// Trade time (ms).
    pub time: i64,
    /// `true` when the buyer was the maker (=> aggressor is the seller).
    #[serde(default, rename = "buyerMaker")]
    pub buyer_maker: bool,
}

// `buyerMaker` in the JSON is camelCase; add the rename explicitly since we don't apply
// rename_all here (the other fields are already lowercase).
impl BingXSpotTrade {
    /// Returns whether the buyer was the maker, reading either `buyerMaker` casing.
    #[must_use]
    pub const fn buyer_is_maker(&self) -> bool {
        self.buyer_maker
    }
}

/// One spot balance entry inside `spot/v1/account/balance` -> `data.balances`.
#[derive(Debug, Clone, Deserialize)]
pub struct BingXSpotBalance {
    pub asset: String,
    #[serde(default)]
    pub free: String,
    #[serde(default)]
    pub locked: String,
}

/// `data` payload of `spot/v1/account/balance`.
#[derive(Debug, Clone, Deserialize)]
pub struct BingXSpotBalances {
    #[serde(default)]
    pub balances: Vec<BingXSpotBalance>,
}

/// `data` payload of `spot/v1/trade/order` (create) — the order acknowledgement.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BingXSpotOrder {
    pub symbol: String,
    #[serde(default)]
    pub order_id: i64,
    #[serde(default, rename = "clientOrderID")]
    pub client_order_id: String,
    #[serde(default)]
    pub price: String,
    #[serde(default)]
    pub orig_qty: String,
    #[serde(default)]
    pub executed_qty: String,
    #[serde(default)]
    pub cummulative_quote_qty: String,
    #[serde(default)]
    pub status: String,
    #[serde(default, rename = "type")]
    pub order_type: String,
    #[serde(default)]
    pub side: String,
    #[serde(default)]
    pub transact_time: i64,
}
