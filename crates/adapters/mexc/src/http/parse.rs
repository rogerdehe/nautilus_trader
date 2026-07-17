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

//! Converts MEXC spot REST models into Nautilus domain types.

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::TradeTick,
    enums::{AggressorSide, LiquiditySide, OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, TradeId, VenueOrderId},
    instruments::{CurrencyPair, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};

use super::models::{MexcBalance, MexcMyTrade, MexcOrder, MexcSymbol, MexcTrade};
use crate::common::{
    consts::MEXC_VENUE,
    enums::{parse_order_status, MexcOrderSide},
    parse::parse_millisecond_timestamp,
};

/// Builds a decimal increment string from a number of decimal places.
///
/// `0 -> "1"`, `1 -> "0.1"`, `3 -> "0.001"`. Mirrors CCXT `parse_precision`.
#[must_use]
pub fn increment_from_decimals(decimals: u32) -> String {
    if decimals == 0 {
        return "1".to_string();
    }
    let mut s = String::from("0.");
    for _ in 0..decimals.saturating_sub(1) {
        s.push('0');
    }
    s.push('1');
    s
}

/// Parses a MEXC spot [`MexcSymbol`] into a Nautilus [`InstrumentAny::CurrencyPair`].
///
/// # Errors
///
/// Returns an error if precision fields are missing or price/size values fail to parse.
pub fn parse_spot_instrument(
    symbol: &MexcSymbol,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::from(symbol.symbol.as_str()), *MEXC_VENUE);
    let raw_symbol = Symbol::from(symbol.symbol.as_str());

    let base_currency = Currency::get_or_create_crypto(&symbol.base_asset);
    let quote_currency = Currency::get_or_create_crypto(&symbol.quote_asset);

    let price_precision = symbol
        .quote_asset_precision
        .ok_or_else(|| anyhow::anyhow!("Missing quoteAssetPrecision for {}", symbol.symbol))?;
    let size_precision = symbol
        .base_asset_precision
        .ok_or_else(|| anyhow::anyhow!("Missing baseAssetPrecision for {}", symbol.symbol))?;

    let price_increment = Price::from(increment_from_decimals(price_precision).as_str());
    let size_increment = Quantity::from(increment_from_decimals(size_precision).as_str());

    let min_quantity = symbol
        .base_size_precision
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(Quantity::from);
    let min_notional = symbol
        .quote_amount_precision
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| Money::new(s.parse::<f64>().unwrap_or(0.0), quote_currency));
    let max_notional = symbol
        .max_quote_amount
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<f64>().ok())
        .map(|v| Money::new(v, quote_currency));

    let maker_fee = symbol
        .maker_commission
        .as_deref()
        .and_then(|s| s.parse().ok());
    let taker_fee = symbol
        .taker_commission
        .as_deref()
        .and_then(|s| s.parse().ok());

    let instrument = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        price_precision as u8,
        size_precision as u8,
        price_increment,
        size_increment,
        None,           // multiplier
        None,           // lot_size
        None,           // max_quantity
        min_quantity,   // min_quantity
        max_notional,   // max_notional
        min_notional,   // min_notional
        None,           // max_price
        None,           // min_price
        None,           // margin_init
        None,           // margin_maint
        maker_fee,      // maker_fee
        taker_fee,      // taker_fee
        None,           // tick_scheme
        None,           // info
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(instrument))
}

/// Parses a MEXC spot recent [`MexcTrade`] into a [`TradeTick`].
///
/// # Errors
///
/// Returns an error if price or quantity fail to parse.
pub fn parse_trade_tick(
    trade: &MexcTrade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = Price::new(trade.price.parse::<f64>()?, price_precision);
    let size = Quantity::new(trade.qty.parse::<f64>()?, size_precision);

    // `is_buyer_maker == true` => the buyer is the maker, so the aggressor is the seller.
    let aggressor_side = match trade.is_buyer_maker {
        Some(true) => AggressorSide::Seller,
        Some(false) => AggressorSide::Buyer,
        None => AggressorSide::NoAggressor,
    };

    let trade_id = TradeId::new(
        trade
            .id
            .clone()
            .unwrap_or_else(|| format!("{}-{}-{}", trade.time, trade.price, trade.qty)),
    );

    let ts_event = parse_millisecond_timestamp(trade.time);

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

/// Parses a MEXC spot [`MexcOrder`] into an [`OrderStatusReport`].
///
/// # Errors
///
/// Returns an error if quantity fields fail to parse.
pub fn parse_order_status_report(
    order: &MexcOrder,
    account_id: AccountId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = InstrumentId::new(Symbol::from(order.symbol.as_str()), *MEXC_VENUE);
    let venue_order_id = VenueOrderId::new(order.order_id.as_str());

    let order_side = match order.side.as_deref() {
        Some("BUY") => OrderSide::Buy,
        Some("SELL") => OrderSide::Sell,
        _ => OrderSide::NoOrderSide,
    };

    let order_type = match order.order_type.as_deref() {
        Some("MARKET") => OrderType::Market,
        _ => OrderType::Limit,
    };

    let order_status = parse_order_status(order.status.as_deref().unwrap_or("NEW"));

    let quantity = Quantity::new(
        order
            .orig_qty
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0),
        size_precision,
    );
    let filled_qty = Quantity::new(
        order
            .executed_qty
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0),
        size_precision,
    );

    let ts_event = order
        .time
        .or(order.transact_time)
        .map_or(ts_init, parse_millisecond_timestamp);
    let ts_last = order
        .update_time
        .map_or(ts_event, parse_millisecond_timestamp);

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None, // client_order_id set below when present
        venue_order_id,
        order_side,
        order_type,
        TimeInForce::Gtc,
        order_status,
        quantity,
        filled_qty,
        ts_event,
        ts_last,
        ts_init,
        None,
    );

    if let Some(price) = order.price.as_deref().and_then(|s| s.parse::<f64>().ok())
        && price > 0.0
    {
        report = report.with_price(Price::new(price, price_precision));
    }
    if let Some(coid) = order
        .client_order_id
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        report = report.with_client_order_id(ClientOrderId::new(coid));
    }

    Ok(report)
}

/// Parses a MEXC spot [`MexcMyTrade`] into a [`FillReport`].
///
/// # Errors
///
/// Returns an error if price or quantity fail to parse.
pub fn parse_fill_report(
    trade: &MexcMyTrade,
    account_id: AccountId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = InstrumentId::new(Symbol::from(trade.symbol.as_str()), *MEXC_VENUE);
    let venue_order_id = VenueOrderId::new(trade.order_id.as_str());
    let trade_id = TradeId::new(trade.id.as_str());

    let order_side = match trade.is_buyer {
        Some(true) => OrderSide::Buy,
        Some(false) => OrderSide::Sell,
        None => OrderSide::NoOrderSide,
    };
    let liquidity_side = match trade.is_maker {
        Some(true) => LiquiditySide::Maker,
        Some(false) => LiquiditySide::Taker,
        None => LiquiditySide::NoLiquiditySide,
    };

    let last_px = Price::new(trade.price.parse::<f64>()?, price_precision);
    let last_qty = Quantity::new(trade.qty.parse::<f64>()?, size_precision);

    let commission_currency = trade
        .commission_asset
        .as_deref()
        .filter(|s| !s.is_empty())
        .map_or_else(
            || Currency::get_or_create_crypto("USDT"),
            Currency::get_or_create_crypto,
        );
    let commission = Money::new(
        trade
            .commission
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0),
        commission_currency,
    );

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
        None, // client_order_id
        None, // venue_position_id
        parse_millisecond_timestamp(trade.time),
        ts_init,
        None,
    ))
}

/// Parses MEXC spot [`MexcBalance`] entries into Nautilus [`AccountBalance`] values.
///
/// Skips zero-balance assets. `free + locked = total`.
#[must_use]
pub fn parse_account_balances(balances: &[MexcBalance]) -> Vec<AccountBalance> {
    let mut out = Vec::new();
    for b in balances {
        let free = b.free.parse::<f64>().unwrap_or(0.0);
        let locked = b.locked.parse::<f64>().unwrap_or(0.0);
        if free == 0.0 && locked == 0.0 {
            continue;
        }
        let currency = Currency::get_or_create_crypto(&b.asset);
        let total = free + locked;
        out.push(AccountBalance::new(
            Money::new(total, currency),
            Money::new(locked, currency),
            Money::new(free, currency),
        ));
    }
    out
}

/// Helper: whether a MEXC symbol is enabled for spot trading (`status == "1"` and spot allowed).
#[must_use]
pub fn is_spot_enabled(symbol: &MexcSymbol) -> bool {
    let status_ok = symbol.status.as_deref() == Some("1");
    let spot_ok = symbol.is_spot_trading_allowed.unwrap_or(true);
    status_ok && spot_ok
}

/// Resolves the [`OrderSide`] wire value for a MEXC spot side string.
#[must_use]
pub fn order_side_str(side: OrderSide) -> Option<&'static str> {
    MexcOrderSide::try_from(side).ok().map(MexcOrderSide::as_str)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use nautilus_model::instruments::Instrument;

    use super::*;
    use crate::http::models::{MexcAccount, MexcDepth, MexcExchangeInfo};

    #[rstest]
    #[case(0, "1")]
    #[case(1, "0.1")]
    #[case(2, "0.01")]
    #[case(4, "0.0001")]
    fn test_increment_from_decimals(#[case] d: u32, #[case] expected: &str) {
        assert_eq!(increment_from_decimals(d), expected);
    }

    #[rstest]
    fn test_parse_exchange_info_fixture() {
        // Fixture from CCXT `fetch_spot_markets` comment block.
        let json = r#"{
            "timezone": "CST",
            "serverTime": 1647521860402,
            "symbols": [{
                "symbol": "OGNUSDT",
                "status": "1",
                "baseAsset": "OGN",
                "baseAssetPrecision": 2,
                "quoteAsset": "USDT",
                "quoteAssetPrecision": 4,
                "quoteOrderQtyMarketAllowed": false,
                "isSpotTradingAllowed": true,
                "baseSizePrecision": "0.01",
                "maxQuoteAmount": "5000000",
                "makerCommission": "0.002",
                "takerCommission": "0.002",
                "quoteAmountPrecision": "5"
            }]
        }"#;
        let info: MexcExchangeInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.symbols.len(), 1);
        let inst = parse_spot_instrument(&info.symbols[0], UnixNanos::default()).unwrap();
        assert_eq!(inst.id().symbol.as_str(), "OGNUSDT");
        assert_eq!(inst.price_precision(), 4);
        assert_eq!(inst.size_precision(), 2);
        assert!(is_spot_enabled(&info.symbols[0]));
    }

    #[rstest]
    fn test_parse_depth_fixture() {
        let json = r#"{
            "lastUpdateId": 744267132,
            "bids": [["40838.50","0.387864"],["40837.95","0.008400"]],
            "asks": [["40838.61","6.544908"],["40838.88","0.498000"]]
        }"#;
        let depth: MexcDepth = serde_json::from_str(json).unwrap();
        assert_eq!(depth.last_update_id, Some(744267132));
        assert_eq!(depth.bids.len(), 2);
        assert_eq!(depth.asks[0][0], "40838.61");
    }

    #[rstest]
    fn test_parse_trade_tick_fixture() {
        // CCXT spot fetchTrades (aggTrades) shape.
        let json = r#"{"p":"40679","q":"0.001309","T":1647551328000,"m":true,"M":true}"#;
        // Remap the terse aggTrades keys onto our MexcTrade (recent-trades) model for the test.
        let trade = MexcTrade {
            price: "40679".to_string(),
            qty: "0.001309".to_string(),
            quote_qty: None,
            time: 1647551328000,
            is_buyer_maker: serde_json::from_str::<serde_json::Value>(json)
                .unwrap()
                .get("m")
                .and_then(|v| v.as_bool()),
            id: None,
        };
        let id = InstrumentId::from("BTCUSDT.MEXC");
        let tick = parse_trade_tick(&trade, id, 2, 6, UnixNanos::default()).unwrap();
        assert_eq!(tick.aggressor_side, AggressorSide::Seller);
        assert_eq!(tick.ts_event.as_u64(), 1_647_551_328_000_000_000);
    }

    #[rstest]
    fn test_parse_order_status_report_fixture() {
        // CCXT spot fetchOrder / cancelOrder shape.
        let json = r#"{
            "symbol": "BTCUSDT",
            "orderId": "133926441921286144",
            "orderListId": "-1",
            "clientOrderId": null,
            "price": "30000",
            "origQty": "0.0002",
            "executedQty": "0",
            "cummulativeQuoteQty": "0",
            "status": "NEW",
            "type": "LIMIT",
            "side": "BUY",
            "time": 1661994066000,
            "updateTime": 1661994066000
        }"#;
        let order: MexcOrder = serde_json::from_str(json).unwrap();
        let account_id = AccountId::from("MEXC-001");
        let report =
            parse_order_status_report(&order, account_id, 2, 6, UnixNanos::default()).unwrap();
        assert_eq!(report.venue_order_id.as_str(), "133926441921286144");
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_status, nautilus_model::enums::OrderStatus::Accepted);
        assert_eq!(report.price.unwrap().as_f64(), 30000.0);
    }

    #[rstest]
    fn test_parse_fill_report_fixture() {
        // CCXT spot fetchMyTrades shape.
        let json = r#"{
            "symbol": "BTCUSDT",
            "id": "133948532984922113",
            "orderId": "133948532531949568",
            "orderListId": "-1",
            "price": "41995.51",
            "qty": "0.0002",
            "quoteQty": "8.399102",
            "commission": "0.016798204",
            "commissionAsset": "USDT",
            "time": 1647718055000,
            "isBuyer": true,
            "isMaker": false,
            "isBestMatch": true
        }"#;
        let trade: MexcMyTrade = serde_json::from_str(json).unwrap();
        let account_id = AccountId::from("MEXC-001");
        let fill = parse_fill_report(&trade, account_id, 2, 6, UnixNanos::default()).unwrap();
        assert_eq!(fill.order_side, OrderSide::Buy);
        assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
        assert_eq!(fill.commission.currency.code.as_str(), "USDT");
        assert_eq!(fill.last_px.as_f64(), 41995.51);
    }

    #[rstest]
    fn test_parse_account_balances_fixture() {
        let json = r#"{"balances":[
            {"asset":"USDT","free":"100.5","locked":"10.0"},
            {"asset":"BTC","free":"0","locked":"0"}
        ]}"#;
        let account: MexcAccount = serde_json::from_str(json).unwrap();
        let balances = parse_account_balances(&account.balances);
        assert_eq!(balances.len(), 1); // zero BTC skipped
        assert_eq!(balances[0].currency.code.as_str(), "USDT");
        assert_eq!(balances[0].total.as_f64(), 110.5);
        assert_eq!(balances[0].free.as_f64(), 100.5);
    }
}
