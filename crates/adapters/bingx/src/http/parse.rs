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

//! Converts BingX REST models into Nautilus domain types.

use std::str::FromStr;

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::TradeTick,
    enums::AggressorSide,
    identifiers::{InstrumentId, Symbol, TradeId},
    instruments::{CurrencyPair, InstrumentAny},
    types::{Currency, Money, Price, Quantity},
};
use crate::common::{consts::bingx_venue, parse::split_base_quote};

/// Formats an `f64` increment/threshold as a plain decimal string (never scientific notation),
/// so it feeds cleanly into `Price::from_str`/`Quantity::from_str` (which derive precision from the
/// decimal places). BingX returns these as JSON numbers; `serde_json` would render small values like
/// `0.000001` as `1e-6`, but Rust's `f64` `Display` always uses shortest round-trip DECIMAL notation
/// (`0.000001`), so the token is directly parseable.
#[must_use]
pub fn f64_to_plain_string(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    value.to_string()
}

fn price_from_f64(value: f64) -> anyhow::Result<Price> {
    Price::from_str(&f64_to_plain_string(value)).map_err(|e| anyhow::anyhow!(e))
}

fn quantity_from_f64(value: f64) -> anyhow::Result<Quantity> {
    Quantity::from_str(&f64_to_plain_string(value)).map_err(|e| anyhow::anyhow!(e))
}

/// Builds an [`InstrumentAny::CurrencyPair`] from a BingX spot market definition.
///
/// # Errors
///
/// Returns an error if the symbol is not `BASE-QUOTE` or the increments cannot be parsed.
#[allow(clippy::too_many_arguments)]
pub fn parse_spot_instrument(
    symbol: &str,
    tick_size: f64,
    step_size: f64,
    min_notional: f64,
    max_notional: f64,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let (base, quote) =
        split_base_quote(symbol).ok_or_else(|| anyhow::anyhow!("Invalid BingX symbol: {symbol}"))?;

    let instrument_id = InstrumentId::new(Symbol::from(symbol), bingx_venue());
    let raw_symbol = Symbol::from(symbol);
    let base_currency = Currency::get_or_create_crypto(&base);
    let quote_currency = Currency::get_or_create_crypto(&quote);

    let price_increment = price_from_f64(tick_size)?;
    let size_increment = quantity_from_f64(step_size)?;

    let min_notional_money = if min_notional > 0.0 {
        Some(Money::new(min_notional, quote_currency))
    } else {
        None
    };
    let max_notional_money = if max_notional > 0.0 {
        Some(Money::new(max_notional, quote_currency))
    } else {
        None
    };

    let pair = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        None, // multiplier
        None, // lot_size
        None, // max_quantity
        None, // min_quantity
        max_notional_money,
        min_notional_money,
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        None, // maker_fee
        None, // taker_fee
        None, // tick_scheme
        None, // info
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(pair))
}

/// Builds a [`TradeTick`] from a BingX spot public trade.
///
/// `buyer_maker == true` means the resting order was a bid, so the aggressor was the seller.
///
/// # Errors
///
/// Returns an error if the price or quantity cannot be parsed.
pub fn parse_spot_trade_tick(
    instrument_id: InstrumentId,
    price: f64,
    qty: f64,
    trade_id: i64,
    ts_event_ms: i64,
    buyer_maker: bool,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = Price::new(price, price_precision);
    let size = Quantity::new(qty, size_precision);
    let aggressor_side = if buyer_maker {
        AggressorSide::Seller
    } else {
        AggressorSide::Buyer
    };
    let trade_id = TradeId::new(&trade_id.to_string());
    let ts_event = UnixNanos::from((ts_event_ms.max(0) as u64) * 1_000_000);

    Ok(TradeTick::new(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    ))
}

#[cfg(test)]
mod tests {
    use nautilus_model::instruments::Instrument;

    use super::*;

    #[test]
    fn plain_string_avoids_scientific() {
        assert_eq!(f64_to_plain_string(0.000001), "0.000001");
        assert_eq!(f64_to_plain_string(1.0), "1");
        assert_eq!(f64_to_plain_string(0.5), "0.5");
    }

    #[test]
    fn builds_spot_currency_pair() {
        let inst =
            parse_spot_instrument("BTC-USDT", 0.01, 0.000001, 5.0, 20000.0, UnixNanos::default())
                .unwrap();
        assert_eq!(inst.id().to_string(), "BTC-USDT.BINGX");
        assert_eq!(inst.price_precision(), 2);
        assert_eq!(inst.size_precision(), 6);
    }

    #[test]
    fn builds_trade_tick() {
        let id = InstrumentId::from("BTC-USDT.BINGX");
        let tick =
            parse_spot_trade_tick(id, 25714.71, 1.674571, 43148253, 1655085975589, false, 2, 6, UnixNanos::default())
                .unwrap();
        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
        assert_eq!(tick.instrument_id, id);
    }
}
