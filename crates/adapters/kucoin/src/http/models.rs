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

//! Serde models mirroring KuCoin REST responses (field names taken first-hand from
//! `ccxt/python/ccxt/kucoin.py`).

use serde::Deserialize;

/// Generic KuCoin response envelope: `{"code":"200000","data":<T>}`.
#[derive(Clone, Debug, Deserialize)]
pub struct KuCoinResponse<T> {
    pub code: String,
    #[serde(default)]
    pub msg: Option<String>,
    pub data: Option<T>,
}

/// A spot symbol definition from `GET /api/v1/symbols`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KuCoinSymbol {
    pub symbol: String,
    pub name: String,
    pub base_currency: String,
    pub quote_currency: String,
    #[serde(default)]
    pub fee_currency: Option<String>,
    #[serde(default)]
    pub market: Option<String>,
    pub base_min_size: String,
    pub quote_min_size: String,
    pub base_max_size: String,
    pub quote_max_size: String,
    pub base_increment: String,
    pub quote_increment: String,
    pub price_increment: String,
    #[serde(default)]
    pub price_limit_rate: Option<String>,
    #[serde(default)]
    pub min_funds: Option<String>,
    #[serde(default)]
    pub is_margin_enabled: bool,
    #[serde(default)]
    pub enable_trading: bool,
}

/// An order book level `[price, size]`.
pub type KuCoinBookLevel = [String; 2];

/// Full order book snapshot from `GET /api/v1/market/orderbook/level2_20|100`.
#[derive(Clone, Debug, Deserialize)]
pub struct KuCoinOrderBook {
    /// Server time in milliseconds.
    #[serde(default)]
    pub time: i64,
    /// Sequence number (string).
    #[serde(default)]
    pub sequence: Option<String>,
    #[serde(default)]
    pub bids: Vec<KuCoinBookLevel>,
    #[serde(default)]
    pub asks: Vec<KuCoinBookLevel>,
}

/// A public trade from `GET /api/v1/market/histories`.
#[derive(Clone, Debug, Deserialize)]
pub struct KuCoinTrade {
    pub sequence: String,
    /// Trade time in NANOSECONDS.
    pub time: i64,
    pub price: String,
    pub size: String,
    /// Aggressor side: `buy` or `sell`.
    pub side: String,
    #[serde(default)]
    pub trade_id: Option<String>,
}

/// A WebSocket bullet token server instance from `POST /api/v1/bullet-public|private`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KuCoinInstanceServer {
    pub endpoint: String,
    #[serde(default)]
    pub encrypt: bool,
    #[serde(default)]
    pub protocol: Option<String>,
    /// Recommended client ping interval in milliseconds.
    pub ping_interval: u64,
    #[serde(default)]
    pub ping_timeout: u64,
}

/// Bullet token response `data` from `POST /api/v1/bullet-public|private`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KuCoinBulletToken {
    pub token: String,
    pub instance_servers: Vec<KuCoinInstanceServer>,
}

/// An account balance entry from `GET /api/v1/accounts`.
#[derive(Clone, Debug, Deserialize)]
pub struct KuCoinAccount {
    #[serde(default)]
    pub id: Option<String>,
    pub currency: String,
    /// Account type: `main`, `trade`, `margin`.
    #[serde(rename = "type")]
    pub account_type: String,
    pub balance: String,
    pub available: String,
    pub holds: String,
}

/// Order placement response `data` from `POST /api/v1/orders`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KuCoinOrderCreated {
    pub order_id: String,
}

/// A spot order object from `GET /api/v1/orders/{orderId}`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KuCoinOrder {
    pub id: String,
    pub symbol: String,
    #[serde(default)]
    pub op_type: Option<String>,
    #[serde(rename = "type")]
    pub order_type: String,
    pub side: String,
    #[serde(default)]
    pub price: Option<String>,
    pub size: String,
    #[serde(default)]
    pub funds: Option<String>,
    #[serde(default)]
    pub deal_funds: Option<String>,
    #[serde(default)]
    pub deal_size: Option<String>,
    #[serde(default)]
    pub client_oid: Option<String>,
    #[serde(default)]
    pub time_in_force: Option<String>,
    #[serde(default)]
    pub post_only: bool,
    #[serde(default)]
    pub cancel_exist: bool,
    #[serde(default)]
    pub is_active: Option<bool>,
    #[serde(default)]
    pub created_at: i64,
}

/// A fill (execution) from `GET /api/v1/fills`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KuCoinFill {
    pub symbol: String,
    pub trade_id: String,
    pub order_id: String,
    pub side: String,
    pub price: String,
    pub size: String,
    #[serde(default)]
    pub funds: Option<String>,
    #[serde(default)]
    pub fee: Option<String>,
    #[serde(default)]
    pub fee_currency: Option<String>,
    #[serde(default)]
    pub liquidity: Option<String>,
    pub created_at: i64,
    #[serde(default)]
    pub client_oid: Option<String>,
}

/// Paginated wrapper `{"items":[...],"currentPage":..,"totalPage":..}` used by some list endpoints.
#[derive(Clone, Debug, Deserialize)]
pub struct KuCoinPaginated<T> {
    #[serde(default = "Vec::new")]
    pub items: Vec<T>,
}
