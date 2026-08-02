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

//! Serde models for LBank v2 spot REST responses.
//!
//! Numeric decimal fields are captured as strings via [`FlexStr`] because LBank returns them as JSON
//! *numbers* over WebSocket (`42585.84`) but as *strings* over some REST endpoints; keeping the raw
//! token avoids an f64 round-trip when building `Price`/`Quantity`.

use serde::{Deserialize, Serialize, de};

/// A decimal value that deserializes from either a JSON string or a JSON number, storing the raw
/// textual token (no f64 round-trip). Used for prices/sizes across REST + WS payloads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlexStr(pub String);

impl FlexStr {
    /// Returns the underlying decimal string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for FlexStr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct V;
        impl de::Visitor<'_> for V {
            type Value = FlexStr;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a decimal string or number")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<FlexStr, E> {
                Ok(FlexStr(v.to_string()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<FlexStr, E> {
                Ok(FlexStr(v))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<FlexStr, E> {
                // Rust's f64 Display is shortest-roundtrip DECIMAL (no scientific notation for the
                // magnitudes seen here), which parses cleanly into `rust_decimal::Decimal`.
                Ok(FlexStr(v.to_string()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<FlexStr, E> {
                Ok(FlexStr(v.to_string()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<FlexStr, E> {
                Ok(FlexStr(v.to_string()))
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// Generic LBank v2 response envelope: `{"result":..,"data":..,"error_code":0,"msg":..}`.
/// `error_code == 0` (or a truthy `result`) means success.
#[derive(Debug, Clone, Deserialize)]
pub struct LBankResponse<T> {
    #[serde(default)]
    pub error_code: i64,
    #[serde(default)]
    pub msg: Option<String>,
    pub data: Option<T>,
}

/// A `[price, qty]` order book level (numbers over WS, strings over REST).
#[derive(Clone, Debug, Deserialize)]
pub struct LBankLevel(pub FlexStr, pub FlexStr);

/// Response of `GET /v2/depth.do` (and the WS `depth` push payload).
#[derive(Clone, Debug, Deserialize)]
pub struct LBankDepth {
    /// Ask levels (ascending price).
    #[serde(default)]
    pub asks: Vec<LBankLevel>,
    /// Bid levels (descending price).
    #[serde(default)]
    pub bids: Vec<LBankLevel>,
}

/// One trade from `GET /v2/supplement/trades.do`.
#[derive(Clone, Debug, Deserialize)]
pub struct LBankRestTrade {
    /// Trade price.
    pub price: FlexStr,
    /// Trade quantity (base).
    pub qty: FlexStr,
    /// Trade time in milliseconds.
    #[serde(default)]
    pub time: Option<i64>,
    /// Trade id.
    #[serde(default)]
    pub id: Option<String>,
    /// `true` when the buyer was the maker (→ taker/aggressor is the seller).
    #[serde(rename = "isBuyerMaker", default)]
    pub is_buyer_maker: Option<bool>,
}

/// One CONTRACT (USDT-perp) instrument from `GET /cfd/openApi/v1/pub/instrument?productGroup=SwapU`.
#[derive(Clone, Debug, Deserialize)]
pub struct LBankContractInstrument {
    /// Contract symbol, e.g. `BTCUSDT` (no underscore).
    pub symbol: String,
    /// Base currency, e.g. `BTC`.
    #[serde(rename = "baseCurrency", default)]
    pub base_currency: Option<String>,
    /// Quote (price) currency, e.g. `USDT`.
    #[serde(rename = "priceCurrency", default)]
    pub price_currency: Option<String>,
    /// Settlement (clearing) currency, e.g. `USDT`.
    #[serde(rename = "clearCurrency", default)]
    pub clear_currency: Option<String>,
    /// Price tick / minimum price increment (e.g. `0.1`). Also the native price-group step used to
    /// build the OrderBook WS subscribe id.
    #[serde(rename = "priceTick", default)]
    pub price_tick: Option<f64>,
    /// Volume tick / minimum size increment (e.g. `0.0001`).
    #[serde(rename = "volumeTick", default)]
    pub volume_tick: Option<f64>,
    /// Contract multiplier (base units per contract), e.g. `1.0`.
    #[serde(rename = "volumeMultiple", default)]
    pub volume_multiple: Option<f64>,
    /// Minimum order volume.
    #[serde(rename = "minOrderVolume", default)]
    pub min_order_volume: Option<FlexStr>,
}

/// Request parameters for `POST /v2/supplement/create_order.do` (business params; the signed system
/// params `api_key`/`echostr`/`signature_method`/`timestamp`/`sign` are added by the client).
#[derive(Clone, Debug)]
pub struct LBankCreateOrderRequest {
    /// LBank pair, e.g. `btc_usdt`.
    pub symbol: String,
    /// Order `type`: `buy`/`sell` (limit) and suffixed variants (`_ioc`/`_fok`/`_maker`/`_market`).
    pub order_type: String,
    /// Limit price (quote).
    pub price: String,
    /// Order amount (base for limit/sell-market; quote spend for buy-market).
    pub amount: String,
    /// Optional client order id (`custom_id`).
    pub custom_id: Option<String>,
}

/// Response `data` of `POST /v2/supplement/create_order.do`.
#[derive(Clone, Debug, Deserialize)]
pub struct LBankCreateOrderResult {
    /// Venue order id.
    #[serde(default)]
    pub order_id: Option<String>,
    /// Echoed client order id.
    #[serde(default)]
    pub custom_id: Option<String>,
    /// Echoed symbol.
    #[serde(default)]
    pub symbol: Option<String>,
}

/// Response `data` of `POST /v2/supplement/cancel_order.do`.
#[derive(Clone, Debug, Deserialize)]
pub struct LBankCancelOrderResult {
    /// Venue order id.
    #[serde(default, alias = "order_id")]
    pub order_id: Option<String>,
    /// Order status code (see `parse_order_status`).
    #[serde(default)]
    pub status: Option<i64>,
}

/// One balance row from `POST /v2/supplement/user_info_account.do` (`data.balances[]`).
#[derive(Clone, Debug, Deserialize)]
pub struct LBankBalance {
    /// Asset code (lowercase, e.g. `usdt`).
    pub asset: String,
    /// Free/available balance.
    #[serde(default)]
    pub free: Option<FlexStr>,
    /// Locked balance.
    #[serde(default)]
    pub locked: Option<FlexStr>,
}

/// Response `data` of `POST /v2/supplement/user_info_account.do`.
#[derive(Clone, Debug, Deserialize)]
pub struct LBankAccount {
    /// Per-asset balances.
    #[serde(default)]
    pub balances: Vec<LBankBalance>,
}

/// Marker so [`LBankCreateOrderRequest`] participates in `Serialize`-generic contexts if needed.
impl Serialize for LBankCreateOrderRequest {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("symbol", &self.symbol)?;
        m.serialize_entry("type", &self.order_type)?;
        m.serialize_entry("price", &self.price)?;
        m.serialize_entry("amount", &self.amount)?;
        if let Some(cid) = &self.custom_id {
            m.serialize_entry("custom_id", cid)?;
        }
        m.end()
    }
}

/// One row of `/v2/accuracy.do` (precision + order-size filters for a spot pair).
#[derive(Debug, Clone, Deserialize)]
pub struct LBankAccuracy {
    /// LBank pair, e.g. `btc_usdt`.
    pub symbol: String,
    /// Price decimal-place count (→ `price_precision`).
    #[serde(rename = "priceAccuracy")]
    pub price_accuracy: String,
    /// Quantity decimal-place count (→ `size_precision`).
    #[serde(rename = "quantityAccuracy")]
    pub quantity_accuracy: String,
    /// Minimum order size in the base currency.
    #[serde(rename = "minTranQua", default)]
    pub min_tran_qua: Option<String>,
    /// Minimum order notional in the quote currency.
    #[serde(rename = "minOrderAmount", default)]
    pub min_order_amount: Option<String>,
}
