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

//! Enumerations for the Bitget adapter (ported first-hand from CCXT `bitget.py`).

use nautilus_model::enums::{OrderSide, OrderStatus, OrderType, TimeInForce};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString};

/// Bitget product type — selects the REST `productType` query / WS `instType` and the
/// nautilus instrument class (spot vs USDT-margined perpetual).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Display, EnumString, AsRefStr)]
pub enum BitgetProductType {
    /// Spot market.
    #[strum(serialize = "SPOT")]
    Spot,
    /// USDT-margined (linear) futures.
    #[strum(serialize = "USDT-FUTURES")]
    UsdtFutures,
    /// Coin-margined (inverse) futures.
    #[strum(serialize = "COIN-FUTURES")]
    CoinFutures,
    /// USDC-margined (linear) futures.
    #[strum(serialize = "USDC-FUTURES")]
    UsdcFutures,
}

impl BitgetProductType {
    /// Returns `true` if this product type is a spot market.
    #[must_use]
    pub fn is_spot(&self) -> bool {
        matches!(self, Self::Spot)
    }
}

/// Bitget order side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BitgetOrderSide {
    Buy,
    Sell,
}

impl From<BitgetOrderSide> for OrderSide {
    fn from(value: BitgetOrderSide) -> Self {
        match value {
            BitgetOrderSide::Buy => OrderSide::Buy,
            BitgetOrderSide::Sell => OrderSide::Sell,
        }
    }
}

impl From<OrderSide> for BitgetOrderSide {
    fn from(value: OrderSide) -> Self {
        match value {
            OrderSide::Sell => BitgetOrderSide::Sell,
            _ => BitgetOrderSide::Buy,
        }
    }
}

/// Bitget order type (`orderType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BitgetOrderType {
    Limit,
    Market,
}

impl From<BitgetOrderType> for OrderType {
    fn from(value: BitgetOrderType) -> Self {
        match value {
            BitgetOrderType::Limit => OrderType::Limit,
            BitgetOrderType::Market => OrderType::Market,
        }
    }
}

/// Bitget time-in-force (`force`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BitgetForce {
    Gtc,
    Ioc,
    Fok,
    PostOnly,
}

impl From<BitgetForce> for TimeInForce {
    fn from(value: BitgetForce) -> Self {
        match value {
            BitgetForce::Ioc => TimeInForce::Ioc,
            BitgetForce::Fok => TimeInForce::Fok,
            // PostOnly is a GTC order with a post-only flag in nautilus.
            BitgetForce::Gtc | BitgetForce::PostOnly => TimeInForce::Gtc,
        }
    }
}

/// Bitget order status (`status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BitgetOrderStatus {
    New,
    Init,
    Live,
    PartialFill,
    PartiallyFilled,
    FullFill,
    Filled,
    Cancelled,
    Canceled,
    Rejected,
    Expired,
}

impl BitgetOrderStatus {
    /// Maps a Bitget order status to the nautilus [`OrderStatus`].
    ///
    /// Mirrors CCXT `parse_order_status`. Note `partial_fill`/`partially_filled` map to
    /// `open` in CCXT; nautilus distinguishes `PartiallyFilled` which is the closer fit.
    #[must_use]
    pub fn as_order_status(&self) -> OrderStatus {
        match self {
            Self::New | Self::Init | Self::Live => OrderStatus::Accepted,
            Self::PartialFill | Self::PartiallyFilled => OrderStatus::PartiallyFilled,
            Self::FullFill | Self::Filled => OrderStatus::Filled,
            Self::Cancelled | Self::Canceled => OrderStatus::Canceled,
            Self::Rejected => OrderStatus::Rejected,
            Self::Expired => OrderStatus::Expired,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(BitgetProductType::Spot, "SPOT")]
    #[case(BitgetProductType::UsdtFutures, "USDT-FUTURES")]
    fn test_product_type_display(#[case] pt: BitgetProductType, #[case] expected: &str) {
        assert_eq!(pt.to_string(), expected);
        assert_eq!(BitgetProductType::from_str(expected).unwrap(), pt);
    }

    #[rstest]
    fn test_order_side_roundtrip() {
        assert_eq!(OrderSide::from(BitgetOrderSide::Buy), OrderSide::Buy);
        assert_eq!(BitgetOrderSide::from(OrderSide::Sell), BitgetOrderSide::Sell);
    }

    #[rstest]
    #[case(BitgetOrderStatus::Live, OrderStatus::Accepted)]
    #[case(BitgetOrderStatus::PartialFill, OrderStatus::PartiallyFilled)]
    #[case(BitgetOrderStatus::Filled, OrderStatus::Filled)]
    #[case(BitgetOrderStatus::Cancelled, OrderStatus::Canceled)]
    fn test_order_status_map(#[case] s: BitgetOrderStatus, #[case] expected: OrderStatus) {
        assert_eq!(s.as_order_status(), expected);
    }
}
