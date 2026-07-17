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

//! Converts Gate REST/WS models into Nautilus domain types.

use std::str::FromStr;

use anyhow::Context;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, TradeTick},
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, Symbol, TradeId},
    instruments::{CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{consts::gate_venue, parse::split_base_quote},
    http::models::{SpotBookLevel, SpotCurrencyPair, SpotOrderBook, SpotTrade},
};

/// Builds an increment string from a number of decimal places (`2` → `"0.01"`, `0` → `"1"`).
#[must_use]
pub fn increment_from_precision(dp: u8) -> String {
    if dp == 0 {
        "1".to_string()
    } else {
        let mut s = String::from("0.");
        for _ in 1..dp {
            s.push('0');
        }
        s.push('1');
        s
    }
}

/// Parses a decimal string into a [`Price`] at the given precision.
pub fn parse_price(value: &str, precision: u8) -> anyhow::Result<Price> {
    let d = Decimal::from_str(value).context(format!("invalid price '{value}'"))?;
    Price::from_decimal_dp(d, precision).context(format!("Price('{value}', {precision})"))
}

/// Parses a decimal string into a [`Quantity`] at the given precision.
pub fn parse_quantity(value: &str, precision: u8) -> anyhow::Result<Quantity> {
    let d = Decimal::from_str(value).context(format!("invalid quantity '{value}'"))?;
    Quantity::from_decimal_dp(d, precision).context(format!("Quantity('{value}', {precision})"))
}

fn parse_fee_pct(value: &Option<String>) -> Option<Decimal> {
    let raw = value.as_ref()?;
    let pct = Decimal::from_str(raw).ok()?;
    Some(pct / Decimal::from(100))
}

/// Parses a Gate spot market into an [`InstrumentAny::CurrencyPair`].
pub fn parse_spot_instrument(
    market: &SpotCurrencyPair,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let raw_symbol = Symbol::new(market.id.to_uppercase());
    let instrument_id = InstrumentId::new(raw_symbol, gate_venue());

    let base_currency = Currency::get_or_create_crypto(&market.base);
    let quote_currency = Currency::get_or_create_crypto(&market.quote);

    let price_precision = market.precision;
    let size_precision = market.amount_precision;

    let price_increment = Price::from(increment_from_precision(price_precision).as_str());
    let size_increment = Quantity::from(increment_from_precision(size_precision).as_str());

    let min_quantity = market
        .min_base_amount
        .as_deref()
        .and_then(|v| parse_quantity(v, size_precision).ok());

    let (min_notional, max_notional) = match split_base_quote(&market.id) {
        Some((_, quote)) => {
            let cur = Currency::get_or_create_crypto(&quote);
            let min = market
                .min_quote_amount
                .as_deref()
                .and_then(|v| Decimal::from_str(v).ok())
                .map(|d| nautilus_model::types::Money::new(f64_from_decimal(d), cur));
            let max = market
                .max_quote_amount
                .as_deref()
                .and_then(|v| Decimal::from_str(v).ok())
                .map(|d| nautilus_model::types::Money::new(f64_from_decimal(d), cur));
            (min, max)
        }
        None => (None, None),
    };

    let taker_fee = parse_fee_pct(&market.fee);
    let maker_fee = parse_fee_pct(&market.maker_fee_rate).or(taker_fee);

    let instrument = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        None, // multiplier
        None, // lot_size
        None, // max_quantity
        min_quantity,
        max_notional,
        min_notional,
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        maker_fee,
        taker_fee,
        None, // tick_scheme
        None, // info
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(instrument))
}

fn f64_from_decimal(d: Decimal) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(0.0)
}

/// Derives event nanos from a Gate spot trade (`create_time_ms` preferred, else `create_time` s).
fn trade_ts_event(trade: &SpotTrade) -> UnixNanos {
    // `create_time_ms` is a string like "1648725035923.0"; take the integer millisecond part and
    // scale to nanos without going through f64 (which loses precision at these magnitudes).
    if let Some(ms) = trade.create_time_ms.as_deref() {
        let int_ms = ms.split('.').next().unwrap_or(ms);
        if let Ok(ms_u) = u64::from_str(int_ms) {
            return UnixNanos::from(ms_u * 1_000_000);
        }
    }
    if let Some(s) = trade.create_time.as_deref() {
        if let Ok(secs) = u64::from_str(s) {
            return UnixNanos::from(secs * 1_000_000_000);
        }
    }
    UnixNanos::default()
}

/// Parses a Gate spot trade into a [`TradeTick`].
pub fn parse_trade_tick(
    trade: &SpotTrade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(&trade.price, price_precision)?;
    let size = parse_quantity(&trade.amount, size_precision)?;
    let aggressor_side = match trade.side.as_str() {
        "buy" => AggressorSide::Buyer,
        "sell" => AggressorSide::Seller,
        _ => AggressorSide::NoAggressor,
    };
    let trade_id = TradeId::new(&trade.id);
    let ts_event = trade_ts_event(trade);

    TradeTick::new_checked(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
}

fn book_side_deltas(
    levels: &[SpotBookLevel],
    side: OrderSide,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    out: &mut Vec<OrderBookDelta>,
) -> anyhow::Result<()> {
    for level in levels {
        let price = parse_price(&level.0, price_precision)?;
        let size = parse_quantity(&level.1, size_precision)?;
        let order = BookOrder::new(side, price, size, 0);
        let flags = RecordFlag::F_SNAPSHOT as u8;
        out.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            flags,
            0,
            ts_event,
            ts_init,
        ));
    }
    Ok(())
}

/// Parses a Gate spot order book snapshot into [`OrderBookDeltas`] (Clear + Adds, last `F_LAST`).
pub fn parse_order_book_snapshot(
    book: &SpotOrderBook,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let mut deltas: Vec<OrderBookDelta> = Vec::with_capacity(book.bids.len() + book.asks.len() + 1);

    let mut clear = OrderBookDelta::clear(instrument_id, 0, ts_event, ts_init);
    clear.flags |= RecordFlag::F_SNAPSHOT as u8;
    deltas.push(clear);

    book_side_deltas(
        &book.bids,
        OrderSide::Buy,
        instrument_id,
        price_precision,
        size_precision,
        ts_event,
        ts_init,
        &mut deltas,
    )?;
    book_side_deltas(
        &book.asks,
        OrderSide::Sell,
        instrument_id,
        price_precision,
        size_precision,
        ts_event,
        ts_init,
        &mut deltas,
    )?;

    if deltas.len() == 1 {
        // Empty book: mark the clear as last.
        deltas[0].flags |= RecordFlag::F_LAST as u8;
    } else if let Some(last) = deltas.last_mut() {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
}

#[cfg(test)]
mod tests {
    use nautilus_model::instruments::Instrument;

    use super::*;

    #[test]
    fn increment_string() {
        assert_eq!(increment_from_precision(0), "1");
        assert_eq!(increment_from_precision(2), "0.01");
        assert_eq!(increment_from_precision(6), "0.000001");
    }

    #[test]
    fn parse_spot_instrument_ok() {
        let market = SpotCurrencyPair {
            id: "BTC_USDT".to_string(),
            base: "BTC".to_string(),
            quote: "USDT".to_string(),
            amount_precision: 4,
            precision: 2,
            min_base_amount: Some("0.0001".to_string()),
            min_quote_amount: Some("1".to_string()),
            max_quote_amount: None,
            fee: Some("0.2".to_string()),
            maker_fee_rate: Some("0.15".to_string()),
            trade_status: Some("tradable".to_string()),
        };
        let inst = parse_spot_instrument(&market, UnixNanos::default()).unwrap();
        assert_eq!(inst.id().to_string(), "BTC_USDT.GATE");
        assert_eq!(inst.price_precision(), 2);
        assert_eq!(inst.size_precision(), 4);
    }

    #[test]
    fn parse_trade_tick_ok() {
        let trade = SpotTrade {
            id: "3130257995".to_string(),
            create_time: Some("1648725035".to_string()),
            create_time_ms: Some("1648725035923.0".to_string()),
            side: "sell".to_string(),
            currency_pair: Some("LTC_USDT".to_string()),
            amount: "0.0116".to_string(),
            price: "130.11".to_string(),
        };
        let id = InstrumentId::from("LTC_USDT.GATE");
        let tick = parse_trade_tick(&trade, id, 2, 4, UnixNanos::default()).unwrap();
        assert_eq!(tick.aggressor_side, AggressorSide::Seller);
        assert_eq!(tick.ts_event.as_u64(), 1_648_725_035_923_000_000_u64);
        assert_eq!(tick.price, Price::from("130.11"));
    }

    #[test]
    fn parse_order_book_snapshot_ok() {
        let book = SpotOrderBook {
            id: Some(1),
            current: Some(1_650_189_272_515),
            update: Some(1_650_189_272_515),
            asks: vec![SpotBookLevel("2.5182".to_string(), "4.199".to_string())],
            bids: vec![SpotBookLevel("2.51518".to_string(), "228.119".to_string())],
        };
        let id = InstrumentId::from("GMT_USDT.GATE");
        let deltas = parse_order_book_snapshot(&book, id, 5, 3, UnixNanos::default(), UnixNanos::default())
            .unwrap();
        // clear + 1 bid + 1 ask
        assert_eq!(deltas.deltas.len(), 3);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        let last = deltas.deltas.last().unwrap();
        assert!(last.flags & RecordFlag::F_LAST as u8 != 0);
    }
}
