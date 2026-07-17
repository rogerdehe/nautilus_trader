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

//! Conversions from Bitget REST/WS models into nautilus domain types.

use std::str::FromStr;

use nautilus_core::{UnixNanos, UUID4};
use nautilus_model::{
    data::TradeTick,
    enums::{AccountType, AggressorSide, OrderStatus, OrderType, TimeInForce},
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, TradeId, VenueOrderId},
    instruments::{CryptoPerpetual, CurrencyPair, InstrumentAny},
    reports::OrderStatusReport,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use super::models::{
    BitgetContractSymbol, BitgetOrder, BitgetSpotAsset, BitgetSpotSymbol, BitgetTrade,
};
use crate::common::{
    enums::{BitgetOrderSide, BitgetOrderStatus, BitgetOrderType, BitgetProductType},
    parse::{instrument_id_from_raw, PERP_SUFFIX},
};

/// Converts a millisecond-epoch string to [`UnixNanos`].
fn millis_str_to_nanos(ms: &str) -> UnixNanos {
    let millis: u64 = ms.parse().unwrap_or(0);
    UnixNanos::from(millis * 1_000_000)
}

/// Parses a textual price to a [`Price`] at the given precision.
pub fn parse_price(value: &str, precision: u8) -> anyhow::Result<Price> {
    let decimal = Decimal::from_str(value)?;
    Price::from_decimal_dp(decimal, precision).map_err(|e| anyhow::anyhow!(e.to_string()))
}

/// Parses a textual quantity to a [`Quantity`] at the given precision.
pub fn parse_quantity(value: &str, precision: u8) -> anyhow::Result<Quantity> {
    let decimal = Decimal::from_str(value)?;
    Quantity::from_decimal_dp(decimal, precision).map_err(|e| anyhow::anyhow!(e.to_string()))
}

fn opt_decimal(value: &str) -> Option<Decimal> {
    if value.is_empty() {
        None
    } else {
        Decimal::from_str(value).ok()
    }
}

/// Builds a [`CurrencyPair`] instrument from a Bitget spot symbol definition.
///
/// Bitget spot precisions are *decimal-place counts*; increments are `10^-precision`
/// (Bitget uses `TICK_SIZE` precision mode in CCXT).
pub fn parse_spot_instrument(
    def: &BitgetSpotSymbol,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = instrument_id_from_raw(&def.symbol, BitgetProductType::Spot);
    let raw_symbol = Symbol::from_str_unchecked(&def.symbol);
    let base_currency = Currency::get_or_create_crypto(&def.base_coin);
    let quote_currency = Currency::get_or_create_crypto(&def.quote_coin);

    let price_precision: u8 = def.price_precision.parse()?;
    let size_precision: u8 = def.quantity_precision.parse()?;
    let price_increment =
        Price::from_decimal_dp(Decimal::new(1, price_precision as u32), price_precision)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let size_increment =
        Quantity::from_decimal_dp(Decimal::new(1, size_precision as u32), size_precision)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let min_quantity = opt_decimal(&def.min_trade_amount)
        .and_then(|d| Quantity::from_decimal_dp(d, size_precision).ok());
    let max_quantity = opt_decimal(&def.max_trade_amount)
        .and_then(|d| Quantity::from_decimal_dp(d, size_precision).ok());
    let min_notional = opt_decimal(&def.min_trade_usdt)
        .map(|d| Money::from_decimal(d, quote_currency))
        .and_then(Result::ok);

    let instrument = CurrencyPair::new(
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
        max_quantity,
        min_quantity,
        None, // max_notional
        min_notional,
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        opt_decimal(&def.maker_fee_rate),
        opt_decimal(&def.taker_fee_rate),
        None, // tick_scheme
        None, // info
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(instrument))
}

/// Builds a [`CryptoPerpetual`] instrument from a Bitget mix (USDT-M) contract definition.
///
/// Only `symbolType == "perpetual"` linear contracts are handled here; delivery/inverse
/// contracts are skipped by the caller.
pub fn parse_perpetual_instrument(
    def: &BitgetContractSymbol,
    product: BitgetProductType,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = instrument_id_from_raw(&def.symbol, product);
    let raw_symbol = Symbol::from_str_unchecked(format!("{}{PERP_SUFFIX}", def.symbol));
    let base_currency = Currency::get_or_create_crypto(&def.base_coin);
    let quote_currency = Currency::get_or_create_crypto(&def.quote_coin);

    // Settlement coin: prefer the first supported margin coin, else the quote.
    let settle_str = def
        .support_margin_coins
        .first()
        .cloned()
        .unwrap_or_else(|| def.quote_coin.clone());
    let settlement_currency = Currency::get_or_create_crypto(&settle_str);
    let is_inverse = base_currency == settlement_currency;

    let price_place: u8 = def.price_place.parse()?;
    let volume_place: u8 = def.volume_place.parse()?;
    // Price tick = priceEndStep interpreted at `price_place` decimals (CCXT Precise reduce).
    let end_step: i64 = if def.price_end_step.is_empty() {
        1
    } else {
        def.price_end_step.parse().unwrap_or(1)
    };
    let price_increment =
        Price::from_decimal_dp(Decimal::new(end_step, price_place as u32), price_place)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let size_increment = if let Some(d) = opt_decimal(&def.size_multiplier) {
        Quantity::from_decimal_dp(d, volume_place).map_err(|e| anyhow::anyhow!(e.to_string()))?
    } else {
        Quantity::from_decimal_dp(Decimal::new(1, volume_place as u32), volume_place)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
    };

    let min_quantity = opt_decimal(&def.min_trade_num)
        .and_then(|d| Quantity::from_decimal_dp(d, volume_place).ok());
    let min_notional = opt_decimal(&def.min_trade_usdt)
        .map(|d| Money::from_decimal(d, quote_currency))
        .and_then(Result::ok);

    let instrument = CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        is_inverse,
        price_increment.precision,
        size_increment.precision,
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
        opt_decimal(&def.maker_fee_rate),
        opt_decimal(&def.taker_fee_rate),
        None, // tick_scheme
        None, // info
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

/// Parses a Bitget public trade into a [`TradeTick`].
pub fn parse_trade_tick(
    trade: &BitgetTrade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(&trade.price, price_precision)?;
    let size = parse_quantity(&trade.size, size_precision)?;
    let aggressor_side = match trade.side.as_str() {
        "buy" => AggressorSide::Buyer,
        "sell" => AggressorSide::Seller,
        _ => AggressorSide::NoAggressor,
    };
    let trade_id = TradeId::new(&trade.trade_id);
    let ts_event = millis_str_to_nanos(&trade.ts);

    TradeTick::new_checked(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
}

/// Parses Bitget spot account assets into an [`AccountState`].
pub fn parse_spot_account_state(
    assets: &[BitgetSpotAsset],
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    let mut balances = Vec::new();
    for a in assets {
        let ccy_str = a.coin.trim();
        if ccy_str.is_empty() {
            continue;
        }
        let currency = Currency::get_or_create_crypto(ccy_str);
        let free = Decimal::from_str(&a.available).unwrap_or_default();
        let frozen = opt_decimal(&a.frozen).unwrap_or_default();
        let locked = opt_decimal(&a.locked).unwrap_or_default();
        let total = free + frozen + locked;
        let free_money = Money::from_decimal(free, currency)?;
        let locked_money = Money::from_decimal(frozen + locked, currency)?;
        let total_money = Money::from_decimal(total, currency)?;
        if let Ok(balance) = AccountBalance::new_checked(total_money, locked_money, free_money) {
            balances.push(balance);
        }
    }

    if balances.is_empty() {
        let zero = Money::new(0.0, Currency::USDT());
        balances.push(AccountBalance::new(zero, zero, zero));
    }

    Ok(AccountState::new(
        account_id,
        AccountType::Cash,
        balances,
        Vec::new(),
        true,
        UUID4::new(),
        ts_init,
        ts_init,
        None,
    ))
}

/// Parses a Bitget order into an [`OrderStatusReport`].
pub fn parse_order_status_report(
    order: &BitgetOrder,
    account_id: AccountId,
    product: BitgetProductType,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument_id_from_raw(&order.symbol, product);
    let venue_order_id = VenueOrderId::new(&order.order_id);
    let client_order_id = if order.client_oid.is_empty() {
        None
    } else {
        Some(ClientOrderId::new(&order.client_oid))
    };

    let order_side: nautilus_model::enums::OrderSide = match order.side.as_str() {
        "sell" => BitgetOrderSide::Sell.into(),
        _ => BitgetOrderSide::Buy.into(),
    };
    let order_type: OrderType = match order.order_type.as_str() {
        "market" => BitgetOrderType::Market.into(),
        _ => BitgetOrderType::Limit.into(),
    };
    let order_status: OrderStatus = parse_status(&order.status);

    let quantity = parse_quantity(
        if order.size.is_empty() { "0" } else { &order.size },
        size_precision,
    )?;
    let filled_qty = parse_quantity(
        if order.base_volume.is_empty() {
            "0"
        } else {
            &order.base_volume
        },
        size_precision,
    )?;
    let ts_accepted = millis_str_to_nanos(&order.c_time);
    let ts_last = millis_str_to_nanos(&order.u_time);

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        order_side,
        order_type,
        TimeInForce::Gtc,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_last,
        ts_init,
        None,
    );

    if order_type == OrderType::Limit
        && !order.price.is_empty()
        && let Ok(price) = parse_price(&order.price, price_precision)
    {
        report = report.with_price(price);
    }

    Ok(report)
}

fn parse_status(status: &str) -> OrderStatus {
    let parsed: Option<BitgetOrderStatus> =
        serde_json::from_value(serde_json::Value::String(status.to_string())).ok();
    parsed
        .map(|s| s.as_order_status())
        .unwrap_or(OrderStatus::Accepted)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::consts::BITGET_VENUE;

    #[rstest]
    fn test_parse_spot_instrument() {
        // Sample lifted from CCXT `fetch_markets` spot docstring.
        let def = BitgetSpotSymbol {
            symbol: "TRXUSDT".to_string(),
            base_coin: "TRX".to_string(),
            quote_coin: "USDT".to_string(),
            min_trade_amount: "0".to_string(),
            max_trade_amount: "10000000000".to_string(),
            taker_fee_rate: "0.002".to_string(),
            maker_fee_rate: "0.002".to_string(),
            price_precision: "6".to_string(),
            quantity_precision: "4".to_string(),
            quote_precision: "6".to_string(),
            status: "online".to_string(),
            min_trade_usdt: "5".to_string(),
        };
        let inst = parse_spot_instrument(&def, UnixNanos::default()).unwrap();
        let InstrumentAny::CurrencyPair(pair) = inst else {
            panic!("expected CurrencyPair");
        };
        assert_eq!(pair.id.symbol.as_str(), "TRXUSDT");
        assert_eq!(pair.id.venue, *BITGET_VENUE);
        assert_eq!(pair.price_precision, 6);
        assert_eq!(pair.size_precision, 4);
        assert_eq!(pair.price_increment.to_string(), "0.000001");
    }

    #[rstest]
    fn test_parse_perpetual_instrument() {
        // Sample lifted from CCXT `fetch_markets` swap docstring.
        let def = BitgetContractSymbol {
            symbol: "BTCUSDT".to_string(),
            base_coin: "BTC".to_string(),
            quote_coin: "USDT".to_string(),
            maker_fee_rate: "0.0002".to_string(),
            taker_fee_rate: "0.0006".to_string(),
            min_trade_num: "0.001".to_string(),
            price_end_step: "1".to_string(),
            volume_place: "3".to_string(),
            price_place: "1".to_string(),
            size_multiplier: "0.001".to_string(),
            symbol_type: "perpetual".to_string(),
            symbol_status: "normal".to_string(),
            min_trade_usdt: "5".to_string(),
            support_margin_coins: vec!["USDT".to_string()],
        };
        let inst =
            parse_perpetual_instrument(&def, BitgetProductType::UsdtFutures, UnixNanos::default())
                .unwrap();
        let InstrumentAny::CryptoPerpetual(perp) = inst else {
            panic!("expected CryptoPerpetual");
        };
        assert_eq!(perp.id.symbol.as_str(), "BTCUSDT-PERP");
        assert!(!perp.is_inverse);
        assert_eq!(perp.price_precision, 1);
        assert_eq!(perp.price_increment.to_string(), "0.1");
        assert_eq!(perp.size_increment.to_string(), "0.001");
    }

    #[rstest]
    fn test_parse_trade_tick() {
        // Sample lifted from CCXT `handle_trades` spot docstring.
        let trade = BitgetTrade {
            trade_id: "1116461060594286593".to_string(),
            price: "43854.01".to_string(),
            size: "0.0535".to_string(),
            side: "buy".to_string(),
            ts: "1701910980366".to_string(),
        };
        let id = instrument_id_from_raw("BTCUSDT", BitgetProductType::Spot);
        let tick = parse_trade_tick(&trade, id, 2, 4, UnixNanos::default()).unwrap();
        assert_eq!(tick.price.to_string(), "43854.01");
        assert_eq!(tick.size.to_string(), "0.0535");
        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
        assert_eq!(tick.ts_event.as_u64(), 1701910980366 * 1_000_000);
    }
}
