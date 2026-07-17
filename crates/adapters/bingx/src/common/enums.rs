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

//! BingX product/enum definitions and conversions to Nautilus enums.
//!
//! Ported first-hand from CCXT (`bingx.py`: `parse_order_status`, order `side`/`type` handling).

use nautilus_model::enums::{OrderSide, OrderStatus, OrderType};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString};

/// BingX product (market) type. BingX serves spot + USDT-M linear swap (and coin-M inverse, not
/// yet wired here).
#[derive(
    Clone, Copy, Debug, Display, PartialEq, Eq, Hash, AsRefStr, EnumString, Serialize, Deserialize,
)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum BingXProductType {
    /// Spot markets (`BTC-USDT`).
    Spot,
    /// USDT-margined perpetual swap (linear).
    Swap,
}

impl BingXProductType {
    /// Returns `true` for the spot product.
    #[must_use]
    pub const fn is_spot(self) -> bool {
        matches!(self, Self::Spot)
    }
}

/// Maps a BingX order `status` string to the Nautilus [`OrderStatus`].
///
/// Mirrors CCXT `parse_order_status` (`NEW`/`PENDING`/`PARTIALLY_FILLED` open, `FILLED` closed,
/// `CANCELED`/`FAILED` canceled). Nautilus distinguishes accepted vs partially-filled, so we map
/// `PARTIALLY_FILLED` to [`OrderStatus::PartiallyFilled`].
#[must_use]
pub fn order_status_from_bingx(status: &str) -> OrderStatus {
    match status {
        "NEW" | "PENDING" | "RUNNING" => OrderStatus::Accepted,
        "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled,
        "FILLED" => OrderStatus::Filled,
        "CANCELED" | "CANCELLED" => OrderStatus::Canceled,
        "FAILED" | "REJECTED" => OrderStatus::Rejected,
        "EXPIRED" => OrderStatus::Expired,
        _ => OrderStatus::Accepted,
    }
}

/// Maps a BingX order `side` string (`BUY`/`SELL`) to the Nautilus [`OrderSide`].
#[must_use]
pub fn order_side_from_bingx(side: &str) -> OrderSide {
    match side.to_uppercase().as_str() {
        "BUY" => OrderSide::Buy,
        "SELL" => OrderSide::Sell,
        _ => OrderSide::NoOrderSide,
    }
}

/// Maps a Nautilus [`OrderSide`] to the BingX `side` string.
#[must_use]
pub fn bingx_side_from_order_side(side: OrderSide) -> &'static str {
    match side {
        OrderSide::Buy => "BUY",
        _ => "SELL",
    }
}

/// Maps a BingX order `type` string to the Nautilus [`OrderType`].
#[must_use]
pub fn order_type_from_bingx(order_type: &str) -> OrderType {
    match order_type.to_uppercase().as_str() {
        "MARKET" => OrderType::Market,
        "LIMIT" => OrderType::Limit,
        "TRIGGER_LIMIT" | "STOP" | "STOP_LIMIT" => OrderType::StopLimit,
        "TRIGGER_MARKET" | "STOP_MARKET" | "TAKE_STOP_MARKET" => OrderType::StopMarket,
        _ => OrderType::Limit,
    }
}

/// Maps a Nautilus [`OrderType`] to the BingX `type` string (spot/swap common subset).
#[must_use]
pub fn bingx_type_from_order_type(order_type: OrderType) -> &'static str {
    match order_type {
        OrderType::Market => "MARKET",
        OrderType::StopMarket => "STOP_MARKET",
        OrderType::StopLimit => "STOP_LIMIT",
        _ => "LIMIT",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_type_roundtrip() {
        assert_eq!(BingXProductType::Spot.as_ref(), "spot");
        assert_eq!("swap".parse::<BingXProductType>().unwrap(), BingXProductType::Swap);
        assert!(BingXProductType::Spot.is_spot());
    }

    #[test]
    fn status_mapping() {
        assert_eq!(order_status_from_bingx("NEW"), OrderStatus::Accepted);
        assert_eq!(order_status_from_bingx("PARTIALLY_FILLED"), OrderStatus::PartiallyFilled);
        assert_eq!(order_status_from_bingx("FILLED"), OrderStatus::Filled);
        assert_eq!(order_status_from_bingx("CANCELED"), OrderStatus::Canceled);
    }

    #[test]
    fn side_and_type_roundtrip() {
        assert_eq!(order_side_from_bingx("BUY"), OrderSide::Buy);
        assert_eq!(bingx_side_from_order_side(OrderSide::Sell), "SELL");
        assert_eq!(order_type_from_bingx("MARKET"), OrderType::Market);
        assert_eq!(bingx_type_from_order_type(OrderType::Limit), "LIMIT");
    }
}
