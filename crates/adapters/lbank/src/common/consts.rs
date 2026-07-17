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

//! LBank constants: venue, hosts, endpoint paths.

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

/// The canonical Nautilus venue string for LBank.
pub const LBANK_VENUE: &str = "LBANK";

/// The adapter/client name used by factories.
pub const LBANK: &str = "LBANK";

/// Returns the [`Venue`] identifier for LBank.
#[must_use]
pub fn lbank_venue() -> Venue {
    Venue::new(Ustr::from(LBANK_VENUE))
}

// Spot REST hosts (mirrors: www.lbkex.net, api.lbank.info).
pub const LBANK_SPOT_HTTP_URL: &str = "https://api.lbkex.com";
/// Spot public WebSocket (full-snapshot depth + trades).
pub const LBANK_SPOT_WS_URL: &str = "wss://www.lbkex.net/ws/V2/";

// Contract (perp) hosts — implemented as a follow-up after spot.
pub const LBANK_CONTRACT_HTTP_URL: &str = "https://lbkperp.lbank.com";
pub const LBANK_CONTRACT_WS_URL: &str = "wss://lbkperpws.lbank.com/ws";

// Spot public endpoints (all suffixed `.do`).
pub const EP_TIMESTAMP: &str = "/v2/timestamp.do";
pub const EP_CURRENCY_PAIRS: &str = "/v2/currencyPairs.do";
pub const EP_ACCURACY: &str = "/v2/accuracy.do";
pub const EP_DEPTH: &str = "/v2/depth.do";
pub const EP_TRADES: &str = "/v2/supplement/trades.do";
pub const EP_KLINE: &str = "/v2/kline.do";

// Spot private endpoints (signed).
pub const EP_CREATE_ORDER: &str = "/v2/supplement/create_order.do";
pub const EP_CANCEL_ORDER: &str = "/v2/supplement/cancel_order.do";
pub const EP_ORDER_INFO: &str = "/v2/spot/trade/orders_info.do";
pub const EP_OPEN_ORDERS: &str = "/v2/supplement/orders_info_no_deal.do";
pub const EP_ACCOUNT: &str = "/v2/supplement/user_info_account.do";

/// `signature_method` value for HMAC-SHA256 keys.
pub const SIGNATURE_METHOD_HMAC: &str = "HmacSHA256";

// WebSocket subscription actions/channels (spot public).
pub const WS_ACTION_SUBSCRIBE: &str = "subscribe";
pub const WS_ACTION_PONG: &str = "pong";
pub const WS_SUBSCRIBE_DEPTH: &str = "depth";
pub const WS_SUBSCRIBE_TRADE: &str = "trade";

/// Default order book snapshot depth requested over WebSocket (LBank allows 10/50/100).
pub const WS_DEPTH_LEVELS: u32 = 100;

/// Default order book snapshot depth requested over REST `depth.do` (1-200).
pub const REST_DEPTH_SIZE: u32 = 100;

/// Default REST recent-trades page size for `supplement/trades.do`.
pub const REST_TRADES_SIZE: u32 = 100;

/// CCXT `describe().rateLimit` for LBank spot (~60 ms/token → ~16 req/s).
pub const LBANK_RATE_LIMIT_MS: u64 = 60;
