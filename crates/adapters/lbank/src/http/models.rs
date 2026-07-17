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

use serde::Deserialize;

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
