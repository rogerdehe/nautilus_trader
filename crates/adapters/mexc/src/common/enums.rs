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

//! MEXC spot enums and mappings to Nautilus enums (first-hand from CCXT `mexc`).

use nautilus_model::enums::{OrderSide, OrderStatus, OrderType, TimeInForce};
use serde::{Deserialize, Serialize};

/// MEXC spot order side.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum MexcOrderSide {
    Buy,
    Sell,
}

impl From<MexcOrderSide> for OrderSide {
    fn from(value: MexcOrderSide) -> Self {
        match value {
            MexcOrderSide::Buy => Self::Buy,
            MexcOrderSide::Sell => Self::Sell,
        }
    }
}

impl MexcOrderSide {
    /// Returns the MEXC wire value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::Sell => "SELL",
        }
    }
}

impl TryFrom<OrderSide> for MexcOrderSide {
    type Error = anyhow::Error;

    fn try_from(value: OrderSide) -> Result<Self, Self::Error> {
        match value {
            OrderSide::Buy => Ok(Self::Buy),
            OrderSide::Sell => Ok(Self::Sell),
            OrderSide::NoOrderSide => anyhow::bail!("Invalid `OrderSide` for MEXC: {value:?}"),
        }
    }
}

/// MEXC spot order type (`type` field, first-hand from CCXT `create_spot_order_request`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MexcOrderType {
    Limit,
    Market,
    LimitMaker,
    ImmediateOrCancel,
    FillOrKill,
}

impl MexcOrderType {
    /// Returns the MEXC wire value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Limit => "LIMIT",
            Self::Market => "MARKET",
            Self::LimitMaker => "LIMIT_MAKER",
            Self::ImmediateOrCancel => "IMMEDIATE_OR_CANCEL",
            Self::FillOrKill => "FILL_OR_KILL",
        }
    }
}

impl From<MexcOrderType> for OrderType {
    fn from(value: MexcOrderType) -> Self {
        match value {
            MexcOrderType::Market => Self::Market,
            // MEXC uses IMMEDIATE_OR_CANCEL / FILL_OR_KILL / LIMIT_MAKER as limit variants.
            MexcOrderType::Limit
            | MexcOrderType::LimitMaker
            | MexcOrderType::ImmediateOrCancel
            | MexcOrderType::FillOrKill => Self::Limit,
        }
    }
}

/// Resolves the MEXC spot order `type` value for a Nautilus order.
///
/// Mirrors CCXT `create_spot_order_request` handling of `postOnly` / `timeInForce`.
#[must_use]
pub fn resolve_mexc_order_type(
    order_type: OrderType,
    time_in_force: TimeInForce,
    post_only: bool,
) -> MexcOrderType {
    match order_type {
        OrderType::Market => MexcOrderType::Market,
        _ => {
            if post_only {
                MexcOrderType::LimitMaker
            } else {
                match time_in_force {
                    TimeInForce::Ioc => MexcOrderType::ImmediateOrCancel,
                    TimeInForce::Fok => MexcOrderType::FillOrKill,
                    _ => MexcOrderType::Limit,
                }
            }
        }
    }
}

/// Maps a MEXC spot order status string to a Nautilus [`OrderStatus`].
///
/// First-hand from CCXT `parse_order_status` (spot statuses only).
#[must_use]
pub fn parse_order_status(status: &str) -> OrderStatus {
    match status {
        "NEW" => OrderStatus::Accepted,
        "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled,
        "FILLED" => OrderStatus::Filled,
        "CANCELED" | "PARTIALLY_CANCELED" => OrderStatus::Canceled,
        _ => OrderStatus::Accepted,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_side_roundtrip() {
        assert_eq!(OrderSide::from(MexcOrderSide::Buy), OrderSide::Buy);
        assert_eq!(MexcOrderSide::try_from(OrderSide::Sell).unwrap(), MexcOrderSide::Sell);
        assert_eq!(MexcOrderSide::Buy.as_str(), "BUY");
    }

    #[rstest]
    #[case(OrderType::Market, TimeInForce::Gtc, false, MexcOrderType::Market)]
    #[case(OrderType::Limit, TimeInForce::Gtc, false, MexcOrderType::Limit)]
    #[case(OrderType::Limit, TimeInForce::Gtc, true, MexcOrderType::LimitMaker)]
    #[case(OrderType::Limit, TimeInForce::Ioc, false, MexcOrderType::ImmediateOrCancel)]
    #[case(OrderType::Limit, TimeInForce::Fok, false, MexcOrderType::FillOrKill)]
    fn test_resolve_order_type(
        #[case] ot: OrderType,
        #[case] tif: TimeInForce,
        #[case] post_only: bool,
        #[case] expected: MexcOrderType,
    ) {
        assert_eq!(resolve_mexc_order_type(ot, tif, post_only), expected);
    }

    #[rstest]
    #[case("NEW", OrderStatus::Accepted)]
    #[case("PARTIALLY_FILLED", OrderStatus::PartiallyFilled)]
    #[case("FILLED", OrderStatus::Filled)]
    #[case("CANCELED", OrderStatus::Canceled)]
    #[case("PARTIALLY_CANCELED", OrderStatus::Canceled)]
    fn test_parse_order_status(#[case] s: &str, #[case] expected: OrderStatus) {
        assert_eq!(parse_order_status(s), expected);
    }
}
