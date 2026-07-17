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

//! Conversions from HashKey REST models to Nautilus domain types.

use std::str::FromStr;

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::TradeTick,
    enums::AggressorSide,
    identifiers::{InstrumentId, Symbol, TradeId},
    instruments::{CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};

use crate::{
    common::parse::instrument_id_from_hashkey_symbol,
    http::models::{HashKeySymbol, HashKeyTrade},
};

/// Parses a HashKey spot symbol definition into a Nautilus [`InstrumentAny::CurrencyPair`].
///
/// Precision, price and size increments come from the `PRICE_FILTER` / `LOT_SIZE` filters (never
/// from parsing the id string, which has no separator).
///
/// # Errors
///
/// Returns an error if the required filters are missing or their values cannot be parsed.
pub fn parse_spot_instrument(
    symbol: &HashKeySymbol,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = instrument_id_from_hashkey_symbol(&symbol.symbol);
    let raw_symbol = Symbol::from(symbol.symbol.as_str());
    let base_currency = Currency::get_or_create_crypto(&symbol.base_asset);
    let quote_currency = Currency::get_or_create_crypto(&symbol.quote_asset);

    let tick_size = symbol
        .filter("PRICE_FILTER")
        .and_then(|f| f.tick_size.as_deref())
        .ok_or_else(|| anyhow::anyhow!("Missing PRICE_FILTER.tickSize for {}", symbol.symbol))?;
    let price_increment = Price::from_str(tick_size)
        .map_err(|e| anyhow::anyhow!("Invalid tickSize '{tick_size}' for {}: {e}", symbol.symbol))?;

    let lot = symbol.filter("LOT_SIZE");
    let step_size = lot
        .and_then(|f| f.step_size.as_deref())
        .ok_or_else(|| anyhow::anyhow!("Missing LOT_SIZE.stepSize for {}", symbol.symbol))?;
    let size_increment = Quantity::from_str(step_size)
        .map_err(|e| anyhow::anyhow!("Invalid stepSize '{step_size}' for {}: {e}", symbol.symbol))?;

    let min_quantity = lot
        .and_then(|f| f.min_qty.as_deref())
        .and_then(|s| Quantity::from_str(s).ok());
    let max_quantity = lot
        .and_then(|f| f.max_qty.as_deref())
        .and_then(|s| Quantity::from_str(s).ok());

    let price_precision = price_increment.precision;
    let size_precision = size_increment.precision;

    let instrument = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        None, // multiplier (spot)
        Some(size_increment),
        max_quantity,
        min_quantity,
        None, // max_notional
        None, // min_notional
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

    Ok(InstrumentAny::CurrencyPair(instrument))
}

/// Parses a HashKey public trade (`GET quote/v1/trades`) into a Nautilus [`TradeTick`].
///
/// HashKey's `ibm` (`isBuyerMaker`) flags the aggressor: `true` => the seller lifted (aggressor
/// SELL), `false` => the buyer lifted (aggressor BUY), matching CCXT `parse_trade`.
///
/// # Errors
///
/// Returns an error if the price or size cannot be parsed.
pub fn parse_public_trade(
    trade: &HashKeyTrade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = Price::from_str(&trade.p)
        .map_err(|e| anyhow::anyhow!("Invalid trade price '{}': {e}", trade.p))?
        .as_f64();
    let price = Price::new(price, price_precision);
    let size = Quantity::from_str(&trade.q)
        .map_err(|e| anyhow::anyhow!("Invalid trade size '{}': {e}", trade.q))?
        .as_f64();
    let size = Quantity::new(size, size_precision);
    let aggressor_side = if trade.ibm {
        AggressorSide::Seller
    } else {
        AggressorSide::Buyer
    };
    let trade_id = TradeId::new(&format!("{}-{}", trade.t, &trade.p));
    let ts_event = UnixNanos::from((trade.t as u64) * 1_000_000);

    TradeTick::new_checked(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use nautilus_model::instruments::Instrument;

    use super::*;
    use crate::http::models::HashKeyExchangeInfo;

    const SAMPLE_EXCHANGE_INFO: &str = r#"{
        "timezone": "UTC",
        "serverTime": "1721661653952",
        "symbols": [
            {
                "symbol": "BTCUSDT",
                "symbolName": "BTCUSDT",
                "status": "TRADING",
                "baseAsset": "BTC",
                "quoteAsset": "USDT",
                "allowMargin": false,
                "filters": [
                    {"minPrice": "0.01", "maxPrice": "100000.0", "tickSize": "0.01", "filterType": "PRICE_FILTER"},
                    {"minQty": "0.00001", "maxQty": "8", "stepSize": "0.00001", "filterType": "LOT_SIZE"},
                    {"minNotional": "1", "filterType": "MIN_NOTIONAL"}
                ]
            }
        ]
    }"#;

    #[test]
    fn parses_spot_instrument_from_exchange_info() {
        let info: HashKeyExchangeInfo = serde_json::from_str(SAMPLE_EXCHANGE_INFO).unwrap();
        assert_eq!(info.symbols.len(), 1);

        let instrument = parse_spot_instrument(&info.symbols[0], UnixNanos::default()).unwrap();
        assert_eq!(instrument.id().to_string(), "BTCUSDT.HASHKEY");
        assert_eq!(instrument.price_precision(), 2);
        assert_eq!(instrument.size_precision(), 5);
    }

    #[test]
    fn parses_public_trade() {
        let trade = HashKeyTrade {
            t: 1_721_682_745_779,
            p: "67835.99".to_string(),
            q: "0.00017".to_string(),
            ibm: true,
        };
        let id = instrument_id_from_hashkey_symbol("BTCUSDT");
        let tick = parse_public_trade(&trade, id, 2, 5, UnixNanos::default()).unwrap();
        assert_eq!(tick.aggressor_side, AggressorSide::Seller);
        assert_eq!(tick.price, Price::new(67835.99, 2));
    }
}
