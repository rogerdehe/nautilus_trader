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

//! BingX constants: venue, REST/WS hosts, endpoint paths, signing headers.
//!
//! All values are ported first-hand from CCXT (`ccxt/python/ccxt/bingx.py::describe`,
//! `ccxt/python/ccxt/pro/bingx.py`). BingX serves spot + USDT-M (linear) swap from a single
//! `open-api.bingx.com/openApi` base; the URL path encodes the product + version + endpoint.

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

/// The canonical Nautilus venue string for BingX.
pub const BINGX: &str = "BINGX";

/// Returns the [`Venue`] identifier for BingX.
#[must_use]
pub fn bingx_venue() -> Venue {
    Venue::new(Ustr::from(BINGX))
}

// REST — single base host for spot + swap (`urls['api']['spot'/'swap']` in CCXT). The `/openApi`
// suffix is part of the base; `sign()` then appends `/<product>/<version>/<path>`.
pub const BINGX_HTTP_BASE_URL: &str = "https://open-api.bingx.com/openApi";

// WebSocket hosts (`urls['api']['ws']` in ccxt pro). Frames are gzip-compressed.
/// Spot public market-data stream.
pub const BINGX_WS_SPOT_URL: &str = "wss://open-api-ws.bingx.com/market";
/// Linear (USDT-M) swap public market-data stream.
pub const BINGX_WS_SWAP_URL: &str = "wss://open-api-swap.bingx.com/swap-market";

// Signing headers (`sign()` in ccxt).
pub const HEADER_API_KEY: &str = "X-BX-APIKEY";
pub const HEADER_SOURCE_KEY: &str = "X-SOURCE-KEY";
/// Broker/source tag echoed by CCXT (`options['broker']`, default `CCXT`).
pub const SOURCE_KEY_VALUE: &str = "NAUTILUS";

// Product path segments used when building the signed URL.
pub const PRODUCT_SPOT: &str = "spot";
pub const PRODUCT_SWAP: &str = "swap";

// Spot endpoints (path relative to `/openApi/spot/<version>/`).
pub const EP_SPOT_SYMBOLS: &str = "spot/v1/common/symbols";
pub const EP_SPOT_DEPTH: &str = "spot/v1/market/depth";
pub const EP_SPOT_TRADES: &str = "spot/v1/market/trades";
pub const EP_SPOT_SERVER_TIME: &str = "spot/v1/server/time";
pub const EP_SPOT_ORDER: &str = "spot/v1/trade/order";
pub const EP_SPOT_CANCEL: &str = "spot/v1/trade/cancel";
pub const EP_SPOT_OPEN_ORDERS: &str = "spot/v1/trade/openOrders";
pub const EP_SPOT_QUERY_ORDER: &str = "spot/v1/trade/query";
pub const EP_SPOT_BALANCE: &str = "spot/v1/account/balance";

// Swap (USDT-M / linear) endpoints (path relative to `/openApi/swap/<version>/`).
pub const EP_SWAP_CONTRACTS: &str = "swap/v2/quote/contracts";
pub const EP_SWAP_DEPTH: &str = "swap/v2/quote/depth";
pub const EP_SWAP_TRADES: &str = "swap/v2/quote/trades";
pub const EP_SWAP_ORDER: &str = "swap/v2/trade/order";
pub const EP_SWAP_BALANCE: &str = "swap/v2/user/balance";

/// Default REST rate limit (`describe().rateLimit` = 100ms between requests → ~10 req/s).
pub const BINGX_REST_RATE_LIMIT_PER_SEC: u32 = 10;

/// Default request timeout (seconds).
pub const BINGX_HTTP_TIMEOUT_SECS: u64 = 60;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn venue_is_bingx() {
        assert_eq!(bingx_venue().to_string(), "BINGX");
    }
}
