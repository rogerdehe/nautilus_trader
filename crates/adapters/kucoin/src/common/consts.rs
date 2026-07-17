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

//! KuCoin constants: venue, hosts, endpoint paths.

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

/// The canonical Nautilus venue string for KuCoin.
pub const KUCOIN_VENUE: &str = "KUCOIN";

/// Returns the [`Venue`] identifier for KuCoin.
#[must_use]
pub fn kucoin_venue() -> Venue {
    Venue::new(Ustr::from(KUCOIN_VENUE))
}

/// Spot REST host (v2/v3 unified under api.kucoin.com; `/api/v1/...` paths).
pub const KUCOIN_HTTP_URL: &str = "https://api.kucoin.com";

/// KuCoin's success response code (string).
pub const KUCOIN_SUCCESS_CODE: &str = "200000";

// ---- Public REST endpoints (path relative to host; the `/api/v1` prefix is applied by the client). ----
pub const EP_SYMBOLS: &str = "/api/v1/symbols";
pub const EP_TIMESTAMP: &str = "/api/v1/timestamp";
pub const EP_ALL_TICKERS: &str = "/api/v1/market/allTickers";
/// Full order book snapshot (top 20) — public, unsigned.
pub const EP_ORDERBOOK_L2_20: &str = "/api/v1/market/orderbook/level2_20";
/// Full order book snapshot (top 100) — public, unsigned.
pub const EP_ORDERBOOK_L2_100: &str = "/api/v1/market/orderbook/level2_100";
/// Recent trade histories — public, unsigned.
pub const EP_TRADE_HISTORIES: &str = "/api/v1/market/histories";
/// Klines/candles — public, unsigned.
pub const EP_CANDLES: &str = "/api/v1/market/candles";
/// WebSocket bullet token (public channels) — POST, unsigned.
pub const EP_BULLET_PUBLIC: &str = "/api/v1/bullet-public";

// ---- Private REST endpoints (signed). ----
/// WebSocket bullet token (private channels) — POST, signed.
pub const EP_BULLET_PRIVATE: &str = "/api/v1/bullet-private";
/// List accounts (balances).
pub const EP_ACCOUNTS: &str = "/api/v1/accounts";
/// High-frequency order placement (spot).
pub const EP_HF_ORDERS: &str = "/api/v1/hf/orders";
/// Standard order placement / query (spot).
pub const EP_ORDERS: &str = "/api/v1/orders";
/// Fills (executions).
pub const EP_FILLS: &str = "/api/v1/fills";

/// Default REST rate limit interval in milliseconds (mirrors CCXT `describe().rateLimit`).
pub const KUCOIN_RATE_LIMIT_MS: u64 = 100;
