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

//! Conversions from KuCoin REST models to Nautilus domain types.

use std::str::FromStr;

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::TradeTick,
    enums::{
        AccountType, AggressorSide, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce,
    },
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    instruments::{CurrencyPair, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use ustr::Ustr;

use super::models::{KuCoinAccount, KuCoinFill, KuCoinOrder, KuCoinSymbol, KuCoinTrade};
use crate::common::parse::instrument_id_from_kucoin_symbol;

/// Parses a decimal string into a [`Price`] using the given precision.
///
/// # Errors
///
/// Returns an error if the string is not a valid decimal.
pub fn parse_price(value: &str, precision: u8) -> anyhow::Result<Price> {
    let dec = rust_decimal::Decimal::from_str(value)
        .map_err(|e| anyhow::anyhow!("Invalid price '{value}': {e}"))?;
    Price::from_decimal_dp(dec, precision).map_err(Into::into)
}

/// Parses a decimal string into a [`Quantity`] using the given precision.
///
/// # Errors
///
/// Returns an error if the string is not a valid decimal.
pub fn parse_quantity(value: &str, precision: u8) -> anyhow::Result<Quantity> {
    let dec = rust_decimal::Decimal::from_str(value)
        .map_err(|e| anyhow::anyhow!("Invalid quantity '{value}': {e}"))?;
    Quantity::from_decimal_dp(dec, precision).map_err(Into::into)
}

/// Maps a KuCoin side string to a Nautilus [`OrderSide`].
#[must_use]
pub fn parse_order_side(side: &str) -> OrderSide {
    match side {
        "buy" => OrderSide::Buy,
        "sell" => OrderSide::Sell,
        _ => OrderSide::NoOrderSide,
    }
}

/// Maps a KuCoin side string to a Nautilus [`AggressorSide`].
#[must_use]
pub fn parse_aggressor_side(side: &str) -> AggressorSide {
    match side {
        "buy" => AggressorSide::Buyer,
        "sell" => AggressorSide::Seller,
        _ => AggressorSide::NoAggressor,
    }
}

/// Maps a KuCoin order type string to a Nautilus [`OrderType`].
#[must_use]
pub fn parse_order_type(order_type: &str) -> OrderType {
    match order_type {
        "market" => OrderType::Market,
        _ => OrderType::Limit,
    }
}

/// Maps a KuCoin time-in-force string to a Nautilus [`TimeInForce`].
#[must_use]
pub fn parse_time_in_force(tif: Option<&str>) -> TimeInForce {
    match tif {
        Some("IOC") => TimeInForce::Ioc,
        Some("FOK") => TimeInForce::Fok,
        _ => TimeInForce::Gtc,
    }
}

/// Parses a KuCoin spot symbol definition into a Nautilus [`InstrumentAny::CurrencyPair`].
///
/// # Errors
///
/// Returns an error if any of the precision/increment fields cannot be parsed.
pub fn parse_spot_instrument(
    def: &KuCoinSymbol,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = instrument_id_from_kucoin_symbol(&def.symbol);
    let raw_symbol = instrument_id.symbol;

    let base_currency =
        Currency::get_or_create_crypto_with_context(Ustr::from(&def.base_currency), Some("kucoin"));
    let quote_currency = Currency::get_or_create_crypto_with_context(
        Ustr::from(&def.quote_currency),
        Some("kucoin"),
    );

    let price_increment = Price::from_str(&def.price_increment)
        .map_err(|e| anyhow::anyhow!("Invalid priceIncrement '{}': {e}", def.price_increment))?;
    let size_increment = Quantity::from_str(&def.base_increment)
        .map_err(|e| anyhow::anyhow!("Invalid baseIncrement '{}': {e}", def.base_increment))?;

    let min_quantity = Quantity::from_str(&def.base_min_size).ok();
    let max_quantity = Quantity::from_str(&def.base_max_size).ok();
    let min_notional = def
        .min_funds
        .as_ref()
        .and_then(|f| Money::from_str(&format!("{f} {}", def.quote_currency)).ok());

    let instrument = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        None,             // multiplier
        None,             // lot_size
        max_quantity,     // max_quantity
        min_quantity,     // min_quantity
        None,             // max_notional
        min_notional,     // min_notional
        None,             // max_price
        None,             // min_price
        None,             // margin_init
        None,             // margin_maint
        None,             // maker_fee
        None,             // taker_fee
        None,             // tick_scheme
        None,             // info
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(instrument))
}

/// Parses a KuCoin public trade into a Nautilus [`TradeTick`].
///
/// KuCoin spot trade `time` is expressed in NANOSECONDS.
///
/// # Errors
///
/// Returns an error if the price/size cannot be parsed or [`TradeTick`] validation fails.
pub fn parse_trade_tick(
    raw: &KuCoinTrade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(&raw.price, price_precision)?;
    let size = parse_quantity(&raw.size, size_precision)?;
    let aggressor = parse_aggressor_side(&raw.side);
    let trade_id = TradeId::new(raw.trade_id.as_deref().unwrap_or(&raw.sequence));
    let ts_event = UnixNanos::from(raw.time as u64);

    TradeTick::new_checked(
        instrument_id,
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
}

/// Derives the Nautilus [`OrderStatus`] from a KuCoin spot order's flags.
#[must_use]
pub fn parse_order_status(order: &KuCoinOrder) -> OrderStatus {
    let filled: f64 = order
        .deal_size
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    if order.cancel_exist {
        return OrderStatus::Canceled;
    }
    match order.is_active {
        Some(true) => {
            if filled > 0.0 {
                OrderStatus::PartiallyFilled
            } else {
                OrderStatus::Accepted
            }
        }
        Some(false) => OrderStatus::Filled,
        None => {
            if filled > 0.0 {
                OrderStatus::PartiallyFilled
            } else {
                OrderStatus::Accepted
            }
        }
    }
}

/// Parses a KuCoin spot order into a Nautilus [`OrderStatusReport`].
///
/// # Errors
///
/// Returns an error if numeric fields cannot be parsed.
pub fn parse_order_status_report(
    order: &KuCoinOrder,
    account_id: AccountId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument_id_from_kucoin_symbol(&order.symbol);
    let client_order_id = order
        .client_oid
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|s| ClientOrderId::new(s.as_str()));
    let venue_order_id = VenueOrderId::new(order.id.as_str());
    let order_side = parse_order_side(&order.side);
    let order_type = parse_order_type(&order.order_type);
    let time_in_force = parse_time_in_force(order.time_in_force.as_deref());
    let order_status = parse_order_status(order);
    let quantity = parse_quantity(&order.size, size_precision)?;
    let filled_qty = parse_quantity(order.deal_size.as_deref().unwrap_or("0"), size_precision)?;
    let ts = UnixNanos::from((order.created_at as u64) * 1_000_000);

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts,
        ts,
        ts_init,
        None,
    );

    if let Some(price) = &order.price
        && let Ok(px) = parse_price(price, price_precision)
        && order_type == OrderType::Limit
    {
        report = report.with_price(px);
    }

    Ok(report)
}

/// Parses a KuCoin fill into a Nautilus [`FillReport`].
///
/// # Errors
///
/// Returns an error if numeric fields cannot be parsed.
pub fn parse_fill_report(
    fill: &KuCoinFill,
    account_id: AccountId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument_id_from_kucoin_symbol(&fill.symbol);
    let venue_order_id = VenueOrderId::new(fill.order_id.as_str());
    let trade_id = TradeId::new(fill.trade_id.as_str());
    let order_side = parse_order_side(&fill.side);
    let last_qty = parse_quantity(&fill.size, size_precision)?;
    let last_px = parse_price(&fill.price, price_precision)?;
    let fee_currency = fill
        .fee_currency
        .as_deref()
        .filter(|s| !s.is_empty())
        .map_or_else(
            || Currency::get_or_create_crypto_with_context(Ustr::from("USDT"), Some("kucoin")),
            |c| Currency::get_or_create_crypto_with_context(Ustr::from(c), Some("kucoin")),
        );
    let fee = fill.fee.as_deref().unwrap_or("0");
    let commission = Money::from_str(&format!("{fee} {}", fee_currency.code))
        .unwrap_or_else(|_| Money::new(0.0, fee_currency));
    let liquidity_side = match fill.liquidity.as_deref() {
        Some("maker") => LiquiditySide::Maker,
        Some("taker") => LiquiditySide::Taker,
        _ => LiquiditySide::NoLiquiditySide,
    };
    let client_order_id = fill
        .client_oid
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|s| ClientOrderId::new(s.as_str()));
    let ts_event = UnixNanos::from((fill.created_at as u64) * 1_000_000);

    Ok(FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        client_order_id,
        None,
        ts_event,
        ts_init,
        None,
    ))
}

/// Builds a Nautilus [`AccountState`] from KuCoin `trade`-type account balances.
///
/// # Errors
///
/// Returns an error if no balances can be constructed.
pub fn parse_account_state(
    accounts: &[KuCoinAccount],
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    let mut balances = Vec::new();
    for acc in accounts.iter().filter(|a| a.account_type == "trade") {
        let currency =
            Currency::get_or_create_crypto_with_context(Ustr::from(&acc.currency), Some("kucoin"));
        let total = match rust_decimal::Decimal::from_str(&acc.balance) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("Skipping balance for {}: {e}", acc.currency);
                continue;
            }
        };
        let free = rust_decimal::Decimal::from_str(&acc.available)
            .unwrap_or(rust_decimal::Decimal::ZERO);
        match AccountBalance::from_total_and_free(total, free, currency) {
            Ok(b) => balances.push(b),
            Err(e) => log::warn!("Invalid balance for {}: {e}", acc.currency),
        }
    }

    if balances.is_empty() {
        let ccy = Currency::USD();
        let zero = Money::new(0.0, ccy);
        balances.push(AccountBalance::new(zero, zero, zero));
    }

    Ok(AccountState::new(
        account_id,
        AccountType::Cash,
        balances,
        vec![],
        true,
        UUID4::new(),
        ts_init,
        ts_init,
        None,
    ))
}
