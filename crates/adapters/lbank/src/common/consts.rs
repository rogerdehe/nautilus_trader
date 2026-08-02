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

// Contract (perp) REST host — public market-data endpoints under `/cfd/openApi/v1/pub`.
pub const LBANK_CONTRACT_HTTP_URL: &str = "https://lbkperp.lbank.com";
// Legacy/execution contract WS (kept for reference; NOT the market-data stream).
pub const LBANK_CONTRACT_WS_URL: &str = "wss://lbkperpws.lbank.com/ws";
/// Contract PUBLIC market-data WebSocket (v3), reverse-engineered from the LBank futures front-end
/// (`cfdWsUrl`). This is the stream the web UI actually consumes for depth + trades. The prod host
/// is deliberately obfuscated by LBank and MAY ROTATE — override via `base_url_ws` if it changes.
pub const LBANK_CONTRACT_WS_V3_URL: &str = "wss://uuws.rerrkvifj.com/ws/v3";

// Contract public REST endpoints (productGroup=SwapU for USDT-margined perps).
pub const EP_CONTRACT_INSTRUMENT: &str = "/cfd/openApi/v1/pub/instrument";
pub const CONTRACT_PRODUCT_GROUP_SWAP_U: &str = "SwapU";

// Contract v3 WS protocol (compact, key-renamed). The subscribe envelope is
// `{"x":<topic>,"a":{"i":"<SYMBOL>"},"z":<type>,"y":<sub_id>}`; pushes are
// `{"d":<data>,"w":<ms>,"x":<topic>,"z":<type>}`. Topics are the numeric `KK` enum.
pub const CONTRACT_WS_TOPIC_MARKET: u8 = 1;
pub const CONTRACT_WS_TOPIC_KLINE: u8 = 2;
pub const CONTRACT_WS_TOPIC_ORDERBOOK: u8 = 3;
pub const CONTRACT_WS_TOPIC_DEAL: u8 = 4;
/// Subscribe (`z`) type: Sub=1, UnSub=0.
pub const CONTRACT_WS_TYPE_SUB: u8 = 1;
pub const CONTRACT_WS_TYPE_UNSUB: u8 = 0;
/// Push (`z`) type on inbound data frames.
pub const CONTRACT_WS_TYPE_PUSH: u8 = 4;
/// Client keepalive interval (seconds); the v3 server drops connections without an app-level ping.
pub const CONTRACT_WS_PING_SECS: u64 = 5;
/// OrderBook level count — LBank's contract depth channel is a TOP-N snapshot fixed at 25 (higher
/// `limit` values return no data). The book is merged at the requested price-group step; passing the
/// instrument's native tick yields the finest book available.
pub const CONTRACT_WS_DEPTH_LIMIT: u32 = 25;

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
