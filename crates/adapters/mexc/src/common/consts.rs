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

//! MEXC constants: venue, hosts, endpoint paths, and timeframe mapping.

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

/// The MEXC venue string.
pub const MEXC: &str = "MEXC";

/// The MEXC [`Venue`].
pub static MEXC_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(MEXC)));

/// The default MEXC spot REST base URL (`describe().urls['api']['spot']`).
pub const MEXC_HTTP_URL: &str = "https://api.mexc.com";

/// The MEXC spot WebSocket base URL (`pro/mexc describe().urls['api']['ws']['spot']`).
///
/// NOTE: all spot public channels here are Protocol Buffers encoded (see crate docs / [`crate::websocket`]).
pub const MEXC_WS_SPOT_URL: &str = "wss://wbs-api.mexc.com/ws";

/// MEXC spot API version segment.
pub const MEXC_API_V3: &str = "/api/v3";

// --- Spot REST endpoint paths (relative to `MEXC_HTTP_URL`) ---------------------------------------

/// Server time endpoint.
pub const EP_TIME: &str = "/api/v3/time";
/// Exchange information (markets) endpoint.
pub const EP_EXCHANGE_INFO: &str = "/api/v3/exchangeInfo";
/// Order book depth endpoint.
pub const EP_DEPTH: &str = "/api/v3/depth";
/// Recent/aggregate trades endpoint.
pub const EP_TRADES: &str = "/api/v3/trades";
/// Aggregate trades endpoint.
pub const EP_AGG_TRADES: &str = "/api/v3/aggTrades";
/// Klines / candlestick endpoint.
pub const EP_KLINES: &str = "/api/v3/klines";
/// 24hr ticker endpoint.
pub const EP_TICKER_24HR: &str = "/api/v3/ticker/24hr";
/// Book ticker endpoint.
pub const EP_BOOK_TICKER: &str = "/api/v3/ticker/bookTicker";
/// New / query / cancel order endpoint (private).
pub const EP_ORDER: &str = "/api/v3/order";
/// Open orders endpoint (private).
pub const EP_OPEN_ORDERS: &str = "/api/v3/openOrders";
/// All orders endpoint (private).
pub const EP_ALL_ORDERS: &str = "/api/v3/allOrders";
/// Account information (balances) endpoint (private).
pub const EP_ACCOUNT: &str = "/api/v3/account";
/// Account trade list endpoint (private).
pub const EP_MY_TRADES: &str = "/api/v3/myTrades";
/// User data stream listen-key endpoint (private).
pub const EP_LISTEN_KEY: &str = "/api/v3/userDataStream";

/// Default receive window (milliseconds) used for signed requests
/// (`options.recvWindow` in CCXT `describe()`).
pub const MEXC_RECV_WINDOW_MS: u64 = 5000;

/// Broker/source identifier sent in the `source` header (CCXT `options.broker`).
pub const MEXC_BROKER_SOURCE: &str = "CCXT";

/// Default rate limit in requests per second (CCXT `rateLimit` = 50ms => 20/s).
pub const MEXC_RATE_LIMIT_PER_SEC: u32 = 20;

/// Maps a Nautilus/CCXT timeframe string to the MEXC spot klines `interval` value.
///
/// From CCXT `options.timeframes.spot`. Returns `None` for unsupported timeframes.
#[must_use]
pub fn spot_kline_interval(timeframe: &str) -> Option<&'static str> {
    Some(match timeframe {
        "1m" => "1m",
        "5m" => "5m",
        "15m" => "15m",
        "30m" => "30m",
        "1h" | "60m" => "60m",
        "4h" => "4h",
        "1d" => "1d",
        "1w" => "1W",
        "1M" => "1M",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_venue() {
        assert_eq!(MEXC_VENUE.as_str(), "MEXC");
    }

    #[rstest]
    #[case("1m", Some("1m"))]
    #[case("1h", Some("60m"))]
    #[case("60m", Some("60m"))]
    #[case("1w", Some("1W"))]
    #[case("1M", Some("1M"))]
    #[case("2h", None)]
    fn test_spot_kline_interval(#[case] tf: &str, #[case] expected: Option<&str>) {
        assert_eq!(spot_kline_interval(tf), expected);
    }
}
