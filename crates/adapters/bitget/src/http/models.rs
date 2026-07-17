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

//! Serde models for Bitget v2 REST responses (ported from CCXT `bitget.py` docstrings).

use serde::{Deserialize, Serialize};

/// Generic Bitget REST response envelope: `{code, msg, requestTime, data}`.
#[derive(Clone, Debug, Deserialize)]
pub struct BitgetResponse<T> {
    pub code: String,
    #[serde(default)]
    pub msg: String,
    #[serde(rename = "requestTime", default)]
    pub request_time: i64,
    #[serde(default = "Option::default")]
    pub data: Option<T>,
}

/// Spot symbol from `GET /api/v2/spot/public/symbols`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitgetSpotSymbol {
    pub symbol: String,
    pub base_coin: String,
    pub quote_coin: String,
    #[serde(default)]
    pub min_trade_amount: String,
    #[serde(default)]
    pub max_trade_amount: String,
    #[serde(default)]
    pub taker_fee_rate: String,
    #[serde(default)]
    pub maker_fee_rate: String,
    /// Number of decimal places for price.
    pub price_precision: String,
    /// Number of decimal places for quantity.
    pub quantity_precision: String,
    #[serde(default)]
    pub quote_precision: String,
    pub status: String,
    #[serde(default)]
    pub min_trade_usdt: String,
}

/// Mix (futures) contract from `GET /api/v2/mix/market/contracts`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitgetContractSymbol {
    pub symbol: String,
    pub base_coin: String,
    pub quote_coin: String,
    #[serde(default)]
    pub maker_fee_rate: String,
    #[serde(default)]
    pub taker_fee_rate: String,
    #[serde(default)]
    pub min_trade_num: String,
    /// Price tick step in the last `price_place` decimal.
    #[serde(default)]
    pub price_end_step: String,
    /// Number of decimal places for quantity.
    pub volume_place: String,
    /// Number of decimal places for price.
    pub price_place: String,
    /// Minimum size step.
    #[serde(default)]
    pub size_multiplier: String,
    #[serde(default)]
    pub symbol_type: String,
    #[serde(default)]
    pub symbol_status: String,
    #[serde(default)]
    pub min_trade_usdt: String,
    #[serde(default)]
    pub support_margin_coins: Vec<String>,
}

/// Public trade fill from `GET /api/v2/spot/market/fills`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitgetTrade {
    pub trade_id: String,
    pub price: String,
    pub size: String,
    pub side: String,
    /// Trade timestamp (millisecond epoch as string).
    pub ts: String,
}

/// Spot account asset from `GET /api/v2/spot/account/assets`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitgetSpotAsset {
    pub coin: String,
    pub available: String,
    #[serde(default)]
    pub frozen: String,
    #[serde(default)]
    pub locked: String,
}

/// Order info from `GET /api/v2/spot/trade/orderInfo` / `unfilled-orders`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitgetOrder {
    pub symbol: String,
    pub order_id: String,
    #[serde(default)]
    pub client_oid: String,
    #[serde(default)]
    pub price: String,
    #[serde(default)]
    pub size: String,
    pub order_type: String,
    pub side: String,
    pub status: String,
    #[serde(default)]
    pub price_avg: String,
    #[serde(default)]
    pub base_volume: String,
    #[serde(rename = "cTime", default)]
    pub c_time: String,
    #[serde(rename = "uTime", default)]
    pub u_time: String,
}

/// Request body for `POST /api/v2/spot/trade/place-order`.
#[derive(Clone, Debug, Serialize)]
pub struct BitgetPlaceOrderRequest {
    pub symbol: String,
    pub side: String,
    #[serde(rename = "orderType")]
    pub order_type: String,
    pub force: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    pub size: String,
    #[serde(rename = "clientOid", skip_serializing_if = "Option::is_none")]
    pub client_oid: Option<String>,
}

/// Response `data` for `POST /api/v2/spot/trade/place-order`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitgetPlaceOrderResponse {
    pub order_id: String,
    #[serde(default)]
    pub client_oid: String,
}

/// Request body for `POST /api/v2/spot/trade/cancel-order`.
#[derive(Clone, Debug, Serialize)]
pub struct BitgetCancelOrderRequest {
    pub symbol: String,
    #[serde(rename = "orderId", skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    #[serde(rename = "clientOid", skip_serializing_if = "Option::is_none")]
    pub client_oid: Option<String>,
}
