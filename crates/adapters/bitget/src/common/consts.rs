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

//! Core constants shared across the Bitget adapter components.

use std::sync::LazyLock;

use nautilus_model::{
    enums::{OrderType, TimeInForce},
    identifiers::{ClientId, Venue},
};
use ustr::Ustr;

/// Venue identifier string.
pub const BITGET: &str = "BITGET";

/// Static venue instance.
pub static BITGET_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(BITGET)));

/// Static client ID instance.
pub static BITGET_CLIENT_ID: LazyLock<ClientId> = LazyLock::new(|| ClientId::new(Ustr::from(BITGET)));

/// Nautilus broker (channel) code sent via `X-CHANNEL-API-CODE` (CCXT `options.broker`).
pub const BITGET_BROKER_ID: &str = "p4sve";

// REST host (CCXT `urls['api']` = `https://api.{hostname}`, hostname `bitget.com`).
pub const BITGET_HTTP_URL: &str = "https://api.bitget.com";

// WebSocket v2 hosts (CCXT `urls['api']['ws']`).
pub const BITGET_WS_PUBLIC_URL: &str = "wss://ws.bitget.com/v2/ws/public";
pub const BITGET_WS_PRIVATE_URL: &str = "wss://ws.bitget.com/v2/ws/private";

/// Request path prefix prepended to every endpoint before signing (CCXT `pathPart = '/api'`).
pub const BITGET_API_PREFIX: &str = "/api";

/// WebSocket application-level ping payload (CCXT `ping()` returns the literal string `ping`).
pub const BITGET_WS_PING: &str = "ping";

/// WebSocket application-level pong payload.
pub const BITGET_WS_PONG: &str = "pong";

/// WebSocket heartbeat (ping) interval in seconds. Bitget disconnects a socket that is idle for
/// 30 seconds, so ping well within that window.
pub const BITGET_WS_HEARTBEAT_SECS: u64 = 20;

/// Bitget REST success response code (`code == "00000"`).
pub const BITGET_SUCCESS_CODE: &str = "00000";

/// Default REST rate limit (requests per second) — conservative shared quota.
pub const BITGET_REST_RATE_LIMIT_PER_SEC: u32 = 10;

/// Bitget supported order time in force.
///
/// Mapped to the `force` parameter on `place-order`: `gtc`/`ioc`/`fok`/`post_only`.
pub const BITGET_SUPPORTED_TIME_IN_FORCE: &[TimeInForce] = &[
    TimeInForce::Gtc,
    TimeInForce::Ioc,
    TimeInForce::Fok,
];

/// Bitget supported order types.
pub const BITGET_SUPPORTED_ORDER_TYPES: &[OrderType] = &[OrderType::Market, OrderType::Limit];
