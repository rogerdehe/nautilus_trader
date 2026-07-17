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

//! KuCoin-specific enumerations and their mappings to/from Nautilus enums.
//!
//! Field/value semantics are taken first-hand from `ccxt/python/ccxt/kucoin.py`
//! (`create_order`, `parse_spot_order`).

use nautilus_model::enums::{OrderSide, OrderType, TimeInForce};
use serde::{Deserialize, Serialize};

/// KuCoin order side (`buy` / `sell`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KuCoinSide {
    Buy,
    Sell,
}

impl KuCoinSide {
    /// Returns the KuCoin wire value.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::Sell => "sell",
        }
    }

    /// Maps to the Nautilus [`OrderSide`].
    #[must_use]
    pub const fn as_order_side(&self) -> OrderSide {
        match self {
            Self::Buy => OrderSide::Buy,
            Self::Sell => OrderSide::Sell,
        }
    }

    /// Constructs from a Nautilus [`OrderSide`], if directional.
    #[must_use]
    pub const fn from_order_side(side: OrderSide) -> Option<Self> {
        match side {
            OrderSide::Buy => Some(Self::Buy),
            OrderSide::Sell => Some(Self::Sell),
            OrderSide::NoOrderSide => None,
        }
    }
}

/// KuCoin order type (`limit` / `market`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KuCoinOrderType {
    Limit,
    Market,
}

impl KuCoinOrderType {
    /// Returns the KuCoin wire value.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Limit => "limit",
            Self::Market => "market",
        }
    }

    /// Maps to the Nautilus [`OrderType`] (only limit/market are supported on the spot path).
    #[must_use]
    pub const fn as_order_type(&self) -> OrderType {
        match self {
            Self::Limit => OrderType::Limit,
            Self::Market => OrderType::Market,
        }
    }
}

/// KuCoin time-in-force (`GTC` / `GTT` / `IOC` / `FOK`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KuCoinTimeInForce {
    GTC,
    GTT,
    IOC,
    FOK,
}

impl KuCoinTimeInForce {
    /// Returns the KuCoin wire value.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::GTC => "GTC",
            Self::GTT => "GTT",
            Self::IOC => "IOC",
            Self::FOK => "FOK",
        }
    }

    /// Maps to the Nautilus [`TimeInForce`] (GTT collapses to GTD-less GTC on the wire).
    #[must_use]
    pub const fn as_time_in_force(&self) -> TimeInForce {
        match self {
            Self::GTC | Self::GTT => TimeInForce::Gtc,
            Self::IOC => TimeInForce::Ioc,
            Self::FOK => TimeInForce::Fok,
        }
    }

    /// Constructs from a Nautilus [`TimeInForce`], if representable on KuCoin spot.
    #[must_use]
    pub const fn from_time_in_force(tif: TimeInForce) -> Option<Self> {
        match tif {
            TimeInForce::Gtc => Some(Self::GTC),
            TimeInForce::Ioc => Some(Self::IOC),
            TimeInForce::Fok => Some(Self::FOK),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn side_roundtrip() {
        assert_eq!(KuCoinSide::Buy.as_order_side(), OrderSide::Buy);
        assert_eq!(KuCoinSide::from_order_side(OrderSide::Sell), Some(KuCoinSide::Sell));
        assert_eq!(KuCoinSide::from_order_side(OrderSide::NoOrderSide), None);
    }

    #[rstest]
    fn tif_maps() {
        assert_eq!(KuCoinTimeInForce::IOC.as_time_in_force(), TimeInForce::Ioc);
        assert_eq!(KuCoinTimeInForce::GTT.as_time_in_force(), TimeInForce::Gtc);
        assert_eq!(KuCoinTimeInForce::from_time_in_force(TimeInForce::Fok), Some(KuCoinTimeInForce::FOK));
    }
}
