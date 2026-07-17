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

//! Serde models for HashKey REST responses (first-hand from CCXT `hashkey.py`).

use serde::{Deserialize, Serialize};

/// A single instrument filter block. HashKey packs heterogeneous rules into one array keyed by
/// `filterType`; all inner fields are optional so a single struct covers every variant.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct HashKeyFilter {
    #[serde(rename = "filterType", default)]
    pub filter_type: String,
    // PRICE_FILTER
    #[serde(rename = "minPrice", default)]
    pub min_price: Option<String>,
    #[serde(rename = "maxPrice", default)]
    pub max_price: Option<String>,
    #[serde(rename = "tickSize", default)]
    pub tick_size: Option<String>,
    // LOT_SIZE
    #[serde(rename = "minQty", default)]
    pub min_qty: Option<String>,
    #[serde(rename = "maxQty", default)]
    pub max_qty: Option<String>,
    #[serde(rename = "stepSize", default)]
    pub step_size: Option<String>,
    // MIN_NOTIONAL
    #[serde(rename = "minNotional", default)]
    pub min_notional: Option<String>,
}

/// A spot symbol definition from `GET api/v1/exchangeInfo` (`symbols[]`).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HashKeySymbol {
    pub symbol: String,
    #[serde(rename = "symbolName", default)]
    pub symbol_name: String,
    #[serde(default)]
    pub status: String,
    #[serde(rename = "baseAsset")]
    pub base_asset: String,
    #[serde(rename = "quoteAsset")]
    pub quote_asset: String,
    #[serde(rename = "allowMargin", default)]
    pub allow_margin: bool,
    #[serde(default)]
    pub filters: Vec<HashKeyFilter>,
}

impl HashKeySymbol {
    /// Returns the filter with the given `filterType`, if present.
    #[must_use]
    pub fn filter(&self, filter_type: &str) -> Option<&HashKeyFilter> {
        self.filters.iter().find(|f| f.filter_type == filter_type)
    }
}

/// The `GET api/v1/exchangeInfo` response (spot markets + coins; contracts ignored for the spot path).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HashKeyExchangeInfo {
    #[serde(default)]
    pub symbols: Vec<HashKeySymbol>,
}

/// The `GET api/v1/time` response.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HashKeyServerTime {
    #[serde(rename = "serverTime")]
    pub server_time: i64,
}

/// The `GET quote/v1/depth` response (`{t, b:[[px,qty]], a:[[px,qty]]}`).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HashKeyDepth {
    pub t: i64,
    #[serde(default)]
    pub b: Vec<[String; 2]>,
    #[serde(default)]
    pub a: Vec<[String; 2]>,
}

/// A single public trade from `GET quote/v1/trades` (`{t, p, q, ibm}`).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HashKeyTrade {
    pub t: i64,
    pub p: String,
    pub q: String,
    /// `isBuyerMaker` — when true the aggressor was the seller.
    #[serde(default)]
    pub ibm: bool,
}

/// A single balance entry from `GET api/v1/account` (`balances[]`).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HashKeyBalance {
    pub asset: String,
    pub total: String,
    pub free: String,
    pub locked: String,
}

/// The `GET api/v1/account` (spot balance) response.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HashKeyAccount {
    #[serde(default)]
    pub balances: Vec<HashKeyBalance>,
}

/// A spot order object returned by create / query / cancel order endpoints.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HashKeyOrder {
    #[serde(rename = "orderId", default)]
    pub order_id: String,
    #[serde(rename = "clientOrderId", default)]
    pub client_order_id: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub price: String,
    #[serde(rename = "origQty", default)]
    pub orig_qty: String,
    #[serde(rename = "executedQty", default)]
    pub executed_qty: String,
    #[serde(default)]
    pub status: String,
    #[serde(rename = "timeInForce", default)]
    pub time_in_force: String,
    #[serde(rename = "type", default)]
    pub order_type: String,
    #[serde(default)]
    pub side: String,
    #[serde(rename = "transactTime", default)]
    pub transact_time: Option<i64>,
    #[serde(default)]
    pub time: Option<i64>,
}

/// The `POST/PUT/DELETE api/v1/userDataStream` listen-key response.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HashKeyListenKey {
    #[serde(rename = "listenKey")]
    pub listen_key: String,
}
