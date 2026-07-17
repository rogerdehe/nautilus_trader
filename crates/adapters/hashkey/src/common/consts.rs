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

//! HashKey Global constants: venue, hosts, endpoint paths.

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

/// The canonical Nautilus venue string for HashKey Global.
pub const HASHKEY_VENUE: &str = "HASHKEY";

/// The default broker / `INPUT-SOURCE` id (CCXT `options.broker`).
pub const HASHKEY_BROKER_ID: &str = "10000700011";

/// Returns the [`Venue`] identifier for HashKey.
#[must_use]
pub fn hashkey_venue() -> Venue {
    Venue::new(Ustr::from(HASHKEY_VENUE))
}

// REST hosts (public + private share the same host per CCXT `urls['api']`).
pub const HASHKEY_HTTP_URL: &str = "https://api-glb.hashkey.com";
pub const HASHKEY_HTTP_TESTNET_URL: &str = "https://api-glb.sim.hashkeydev.com";

// WebSocket hosts.
pub const HASHKEY_WS_PUBLIC_URL: &str = "wss://stream-glb.hashkey.com/quote/ws/v1";
pub const HASHKEY_WS_PRIVATE_URL: &str = "wss://stream-glb.hashkey.com/api/v1/ws";
pub const HASHKEY_WS_PUBLIC_TESTNET_URL: &str = "wss://stream-glb.sim.hashkeydev.com/quote/ws/v1";
pub const HASHKEY_WS_PRIVATE_TESTNET_URL: &str = "wss://stream-glb.sim.hashkeydev.com/api/v1/ws";

// Public REST endpoints.
pub const EP_PING: &str = "api/v1/ping";
pub const EP_TIME: &str = "api/v1/time";
pub const EP_EXCHANGE_INFO: &str = "api/v1/exchangeInfo";
pub const EP_DEPTH: &str = "quote/v1/depth";
pub const EP_TRADES: &str = "quote/v1/trades";
pub const EP_KLINES: &str = "quote/v1/klines";
pub const EP_TICKER_24HR: &str = "quote/v1/ticker/24hr";
pub const EP_TICKER_BOOK: &str = "quote/v1/ticker/bookTicker";

// Private REST endpoints (spot).
pub const EP_ACCOUNT: &str = "api/v1/account";
pub const EP_SPOT_ORDER: &str = "api/v1/spot/order";
pub const EP_SPOT_OPEN_ORDERS: &str = "api/v1/spot/openOrders";
pub const EP_SPOT_TRADE_ORDERS: &str = "api/v1/spot/tradeOrders";
pub const EP_ACCOUNT_TRADES: &str = "api/v1/account/trades";
pub const EP_USER_DATA_STREAM: &str = "api/v1/userDataStream";

/// Default rate limit interval (CCXT `describe().rateLimit` = 100ms).
pub const HASHKEY_RATE_LIMIT_MS: u64 = 100;

/// The `-PERPETUAL` suffix used by HashKey swap market ids (e.g. `BTCUSDT-PERPETUAL`).
pub const PERPETUAL_SUFFIX: &str = "-PERPETUAL";
