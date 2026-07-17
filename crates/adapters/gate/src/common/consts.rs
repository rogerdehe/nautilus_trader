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

//! Gate (gate.io) constants: venue, hosts, endpoint paths, WS channels.

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

/// The canonical Nautilus venue string for Gate.
pub const GATE_VENUE: &str = "GATE";

/// The adapter/client name used by factories.
pub const GATE: &str = "GATE";

/// Returns the [`Venue`] identifier for Gate.
#[must_use]
pub fn gate_venue() -> Venue {
    Venue::new(Ustr::from(GATE_VENUE))
}

/// REST base host (shared by all product types under `/api/v4`), per CCXT `urls.api`.
pub const GATE_HTTP_BASE_URL: &str = "https://api.gateio.ws";
/// REST API version prefix.
pub const GATE_API_VERSION: &str = "v4";
/// REST signing path prefix (`/api/v4`).
pub const GATE_SIGN_PREFIX: &str = "/api/v4";

/// Spot public + private WebSocket (per CCXT pro `urls.api.spot`).
pub const GATE_SPOT_WS_URL: &str = "wss://api.gateio.ws/ws/v4/";
/// USDT-settled perpetual futures WebSocket (per CCXT pro `urls.api.swap.usdt`).
pub const GATE_FUTURES_USDT_WS_URL: &str = "wss://fx-ws.gateio.ws/v4/ws/usdt";

// REST product path segments (the `type` element of CCXT's `api[1]`).
pub const GATE_TYPE_SPOT: &str = "spot";
pub const GATE_TYPE_FUTURES: &str = "futures";

// Spot REST endpoint paths (relative to `/api/v4/spot`).
pub const EP_SPOT_CURRENCY_PAIRS: &str = "currency_pairs";
pub const EP_SPOT_TICKERS: &str = "tickers";
pub const EP_SPOT_ORDER_BOOK: &str = "order_book";
pub const EP_SPOT_TRADES: &str = "trades";
pub const EP_SPOT_ORDERS: &str = "orders";
pub const EP_SPOT_ACCOUNTS: &str = "accounts";
pub const EP_SPOT_OPEN_ORDERS: &str = "open_orders";

// WebSocket channels (spot).
pub const CH_SPOT_ORDER_BOOK: &str = "spot.order_book";
pub const CH_SPOT_TRADES: &str = "spot.trades";
pub const CH_SPOT_PING: &str = "spot.ping";
pub const CH_SPOT_PONG: &str = "spot.pong";

/// Default order book snapshot depth (levels) requested over WebSocket. Gate allows 5/10/20/50/100.
pub const GATE_WS_BOOK_LEVELS: &str = "20";
/// Default order book snapshot interval over WebSocket (100ms or 1000ms).
pub const GATE_WS_BOOK_INTERVAL: &str = "100ms";

/// CCXT `describe().rateLimit` (ms per request token).
pub const GATE_RATE_LIMIT_MS: u64 = 50;
