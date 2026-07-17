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

//! Serde models for MEXC spot v3 REST responses (first-hand from CCXT `mexc`).

use serde::Deserialize;

/// `GET /api/v3/exchangeInfo` response.
#[derive(Clone, Debug, Deserialize)]
pub struct MexcExchangeInfo {
    /// The list of spot symbols.
    pub symbols: Vec<MexcSymbol>,
}

/// A single spot market from `exchangeInfo.symbols`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MexcSymbol {
    /// The market id (e.g. `BTCUSDT`).
    pub symbol: String,
    /// Trading status (`"1"` = enabled).
    #[serde(default)]
    pub status: Option<String>,
    /// The base asset code (e.g. `BTC`).
    pub base_asset: String,
    /// The quote asset code (e.g. `USDT`).
    pub quote_asset: String,
    /// Number of decimal places for the base asset (size precision).
    #[serde(default)]
    pub base_asset_precision: Option<u32>,
    /// Number of decimal places for the quote asset (price precision).
    #[serde(default)]
    pub quote_asset_precision: Option<u32>,
    /// Minimum base order amount (string decimal), e.g. `"0.01"`.
    #[serde(default)]
    pub base_size_precision: Option<String>,
    /// Minimum quote (cost) order amount (string decimal).
    #[serde(default)]
    pub quote_amount_precision: Option<String>,
    /// Maximum quote (cost) order amount.
    #[serde(default)]
    pub max_quote_amount: Option<String>,
    /// Whether spot trading is allowed.
    #[serde(default)]
    pub is_spot_trading_allowed: Option<bool>,
    /// Maker commission rate.
    #[serde(default)]
    pub maker_commission: Option<String>,
    /// Taker commission rate.
    #[serde(default)]
    pub taker_commission: Option<String>,
}

/// `GET /api/v3/depth` response.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MexcDepth {
    /// Order book update id.
    #[serde(default)]
    pub last_update_id: Option<u64>,
    /// Bid levels: `[price, quantity]`.
    pub bids: Vec<[String; 2]>,
    /// Ask levels: `[price, quantity]`.
    pub asks: Vec<[String; 2]>,
    /// Optional server timestamp (ms).
    #[serde(default)]
    pub timestamp: Option<i64>,
}

/// An element of the `GET /api/v3/trades` array (spot recent trades).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MexcTrade {
    /// Trade price.
    pub price: String,
    /// Trade quantity (base).
    pub qty: String,
    /// Quote quantity.
    #[serde(default)]
    pub quote_qty: Option<String>,
    /// Trade time (ms).
    pub time: i64,
    /// Whether the buyer is the maker (true => aggressor is the seller).
    #[serde(default)]
    pub is_buyer_maker: Option<bool>,
    /// Trade id (may be absent for aggregated trades).
    #[serde(default)]
    pub id: Option<String>,
}

/// `GET /api/v3/account` response (spot balances).
#[derive(Clone, Debug, Deserialize)]
pub struct MexcAccount {
    /// The per-asset balances.
    pub balances: Vec<MexcBalance>,
}

/// A single spot asset balance.
#[derive(Clone, Debug, Deserialize)]
pub struct MexcBalance {
    /// The asset code (e.g. `USDT`).
    pub asset: String,
    /// The free (available) amount.
    pub free: String,
    /// The locked (in-order) amount.
    pub locked: String,
}

/// A spot order object (create / cancel / query), first-hand from CCXT `parse_order`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MexcOrder {
    /// The market id.
    pub symbol: String,
    /// The exchange order id.
    pub order_id: String,
    /// The client order id (may be null).
    #[serde(default)]
    pub client_order_id: Option<String>,
    /// The order price.
    #[serde(default)]
    pub price: Option<String>,
    /// The original order quantity.
    #[serde(default)]
    pub orig_qty: Option<String>,
    /// The executed (filled) quantity.
    #[serde(default)]
    pub executed_qty: Option<String>,
    /// The cumulative quote quantity filled.
    #[serde(default)]
    pub cummulative_quote_qty: Option<String>,
    /// The order status (e.g. `NEW`, `FILLED`).
    #[serde(default)]
    pub status: Option<String>,
    /// The order type (e.g. `LIMIT`).
    #[serde(default, rename = "type")]
    pub order_type: Option<String>,
    /// The order side (`BUY`/`SELL`).
    #[serde(default)]
    pub side: Option<String>,
    /// The creation time (ms).
    #[serde(default)]
    pub time: Option<i64>,
    /// The last update time (ms).
    #[serde(default)]
    pub update_time: Option<i64>,
    /// The transaction time for newly-created orders (ms).
    #[serde(default)]
    pub transact_time: Option<i64>,
}

/// A spot account trade (fill), first-hand from CCXT `parse_trade` (fetchMyTrades shape).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MexcMyTrade {
    /// The market id.
    pub symbol: String,
    /// The trade id.
    pub id: String,
    /// The order id this fill belongs to.
    pub order_id: String,
    /// The fill price.
    pub price: String,
    /// The fill quantity (base).
    pub qty: String,
    /// The quote quantity.
    #[serde(default)]
    pub quote_qty: Option<String>,
    /// The commission amount.
    #[serde(default)]
    pub commission: Option<String>,
    /// The commission asset.
    #[serde(default)]
    pub commission_asset: Option<String>,
    /// The fill time (ms).
    pub time: i64,
    /// Whether this account was the buyer.
    #[serde(default)]
    pub is_buyer: Option<bool>,
    /// Whether this account was the maker.
    #[serde(default)]
    pub is_maker: Option<bool>,
}

/// `POST /api/v3/userDataStream` listen-key response.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MexcListenKey {
    /// The listen key for the user data WebSocket stream.
    pub listen_key: String,
}
