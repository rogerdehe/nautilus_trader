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

//! Parse LBank REST responses into Nautilus domain types.

use std::str::FromStr;

use anyhow::Context;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, TradeTick},
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, Symbol, TradeId},
    instruments::{CurrencyPair, InstrumentAny},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::parse::{instrument_id_from_lbank_symbol, split_base_quote},
    http::models::{LBankAccuracy, LBankDepth, LBankLevel, LBankRestTrade},
};

/// Parses a decimal string into a [`Price`] at the given precision.
pub fn parse_price(value: &str, precision: u8) -> anyhow::Result<Price> {
    let d = Decimal::from_str(value).with_context(|| format!("invalid price '{value}'"))?;
    Price::from_decimal_dp(d, precision).with_context(|| format!("Price('{value}', {precision})"))
}

/// Parses a decimal string into a [`Quantity`] at the given precision.
pub fn parse_quantity(value: &str, precision: u8) -> anyhow::Result<Quantity> {
    let d = Decimal::from_str(value).with_context(|| format!("invalid quantity '{value}'"))?;
    Quantity::from_decimal_dp(d, precision)
        .with_context(|| format!("Quantity('{value}', {precision})"))
}

/// Maps an LBank order/trade `direction` (`buy`, `sell`, `buy_market`, `sell_maker`, ...) to the
/// taker [`AggressorSide`]. The label names the MAKER's side when suffixed `_maker`, so that case
/// is flipped (per CCXT `parse_ws_trade`).
#[must_use]
pub fn aggressor_from_direction(direction: &str) -> AggressorSide {
    let mut parts = direction.split('_');
    let prefix = parts.next().unwrap_or("");
    let suffix = parts.next();
    let mut side = match prefix {
        "buy" => AggressorSide::Buyer,
        "sell" => AggressorSide::Seller,
        _ => return AggressorSide::NoAggressor,
    };
    if suffix == Some("maker") {
        side = match side {
            AggressorSide::Buyer => AggressorSide::Seller,
            _ => AggressorSide::Buyer,
        };
    }
    side
}

/// Builds a spot [`InstrumentAny::CurrencyPair`] from one `/v2/accuracy.do` row.
pub fn instrument_from_accuracy(
    acc: &LBankAccuracy,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = instrument_id_from_lbank_symbol(&acc.symbol);
    let raw_symbol = Symbol::from(acc.symbol.as_str());
    let (base, quote) = split_base_quote(&acc.symbol)
        .with_context(|| format!("invalid LBank symbol '{}'", acc.symbol))?;
    let base_currency = Currency::get_or_create_crypto(&base);
    let quote_currency = Currency::get_or_create_crypto(&quote);

    let price_precision: u8 = acc
        .price_accuracy
        .trim()
        .parse()
        .with_context(|| format!("priceAccuracy '{}'", acc.price_accuracy))?;
    let size_precision: u8 = acc
        .quantity_accuracy
        .trim()
        .parse()
        .with_context(|| format!("quantityAccuracy '{}'", acc.quantity_accuracy))?;

    let price_increment = Price::new(10f64.powi(-(price_precision as i32)), price_precision);
    let size_increment = Quantity::new(10f64.powi(-(size_precision as i32)), size_precision);

    let min_quantity = acc
        .min_tran_qua
        .as_ref()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map(|q| Quantity::new(q, size_precision));
    let min_notional = acc
        .min_order_amount
        .as_ref()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map(|n| Money::new(n, quote_currency));

    let cp = CurrencyPair::new(
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
        None, // max_notional
        min_notional,
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
    Ok(InstrumentAny::CurrencyPair(cp))
}

fn book_side_deltas(
    levels: &[LBankLevel],
    side: OrderSide,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    out: &mut Vec<OrderBookDelta>,
) -> anyhow::Result<()> {
    for level in levels {
        let price = parse_price(level.0.as_str(), price_precision)?;
        let size = parse_quantity(level.1.as_str(), size_precision)?;
        let order = BookOrder::new(side, price, size, 0);
        out.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            RecordFlag::F_SNAPSHOT as u8,
            0,
            ts_event,
            ts_init,
        ));
    }
    Ok(())
}

/// Parses an LBank depth payload (full snapshot) into [`OrderBookDeltas`]: a `Clear` (flagged
/// `F_SNAPSHOT`) followed by `Add` deltas for every level (asks=Sell, bids=Buy), the last carrying
/// `F_LAST`.
pub fn parse_depth_snapshot(
    depth: &LBankDepth,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let mut deltas: Vec<OrderBookDelta> =
        Vec::with_capacity(depth.bids.len() + depth.asks.len() + 1);

    let mut clear = OrderBookDelta::clear(instrument_id, 0, ts_event, ts_init);
    clear.flags |= RecordFlag::F_SNAPSHOT as u8;
    deltas.push(clear);

    book_side_deltas(
        &depth.bids,
        OrderSide::Buy,
        instrument_id,
        price_precision,
        size_precision,
        ts_event,
        ts_init,
        &mut deltas,
    )?;
    book_side_deltas(
        &depth.asks,
        OrderSide::Sell,
        instrument_id,
        price_precision,
        size_precision,
        ts_event,
        ts_init,
        &mut deltas,
    )?;

    if let Some(last) = deltas.last_mut() {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
}

/// Parses one `supplement/trades.do` row into a [`TradeTick`]. Aggressor comes from `isBuyerMaker`
/// (buyer-maker → taker is the seller); trade id from `id` or a synthesized `TS`-based fallback.
pub fn parse_rest_trade_tick(
    trade: &LBankRestTrade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(trade.price.as_str(), price_precision)?;
    let size = parse_quantity(trade.qty.as_str(), size_precision)?;
    let aggressor_side = match trade.is_buyer_maker {
        Some(true) => AggressorSide::Seller,
        Some(false) => AggressorSide::Buyer,
        None => AggressorSide::NoAggressor,
    };
    let ts_event = trade
        .time
        .filter(|ms| *ms > 0)
        .map_or(ts_init, |ms| UnixNanos::from(ms as u64 * 1_000_000));
    let trade_id = match &trade.id {
        Some(id) if !id.is_empty() => TradeId::new(id),
        _ => TradeId::new(format!("{}-{}", ts_event.as_u64(), trade.price.as_str()).as_str()),
    };

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::models::FlexStr;

    #[test]
    fn aggressor_direction_mapping() {
        assert_eq!(aggressor_from_direction("buy"), AggressorSide::Buyer);
        assert_eq!(aggressor_from_direction("sell"), AggressorSide::Seller);
        assert_eq!(aggressor_from_direction("sell_market"), AggressorSide::Seller);
        // `_maker` names the maker's side → aggressor is the opposite.
        assert_eq!(aggressor_from_direction("buy_maker"), AggressorSide::Seller);
        assert_eq!(aggressor_from_direction("sell_maker"), AggressorSide::Buyer);
        assert_eq!(aggressor_from_direction("buy_ioc"), AggressorSide::Buyer);
    }

    #[test]
    fn parse_depth_snapshot_ok() {
        let depth = LBankDepth {
            asks: vec![LBankLevel(FlexStr("42585.84".into()), FlexStr("1.4422".into()))],
            bids: vec![LBankLevel(FlexStr("42585.83".into()), FlexStr("1.8054".into()))],
        };
        let id = InstrumentId::from("BTC_USDT.LBANK");
        let deltas =
            parse_depth_snapshot(&depth, id, 2, 4, UnixNanos::default(), UnixNanos::default())
                .unwrap();
        assert_eq!(deltas.deltas.len(), 3); // clear + 1 bid + 1 ask
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[2].order.side, OrderSide::Sell);
        assert!(deltas.deltas.last().unwrap().flags & RecordFlag::F_LAST as u8 != 0);
        assert!(deltas.deltas[0].flags & RecordFlag::F_SNAPSHOT as u8 != 0);
    }

    #[test]
    fn parse_rest_trade_ok() {
        let trade = LBankRestTrade {
            price: FlexStr("0.127545".into()),
            qty: FlexStr("13133".into()),
            time: Some(1_648_058_297_110),
            id: Some("3589541dc22e4357b227283650f714e2".into()),
            is_buyer_maker: Some(false),
        };
        let id = InstrumentId::from("BTC_USDT.LBANK");
        let tick = parse_rest_trade_tick(&trade, id, 6, 0, UnixNanos::default()).unwrap();
        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
        assert_eq!(tick.ts_event.as_u64(), 1_648_058_297_110_000_000);
        assert_eq!(tick.price, Price::from("0.127545"));
    }

    #[test]
    fn parse_btc_usdt_instrument() {
        let acc = LBankAccuracy {
            symbol: "btc_usdt".to_string(),
            price_accuracy: "2".to_string(),
            quantity_accuracy: "4".to_string(),
            min_tran_qua: Some("0.001".to_string()),
            min_order_amount: Some("5".to_string()),
        };
        let inst = instrument_from_accuracy(&acc, UnixNanos::default()).unwrap();
        match inst {
            InstrumentAny::CurrencyPair(cp) => {
                assert_eq!(cp.id.to_string(), "BTC_USDT.LBANK");
                assert_eq!(cp.raw_symbol.as_str(), "btc_usdt");
                assert_eq!(cp.price_precision, 2);
                assert_eq!(cp.size_precision, 4);
                assert_eq!(cp.base_currency.code.as_str(), "BTC");
                assert_eq!(cp.quote_currency.code.as_str(), "USDT");
                assert!(cp.min_notional.is_some());
            }
            _ => panic!("expected CurrencyPair"),
        }
    }
}
