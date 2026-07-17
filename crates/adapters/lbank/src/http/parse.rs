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

use anyhow::Context;
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::Symbol,
    instruments::{CurrencyPair, InstrumentAny},
    types::{Currency, Money, Price, Quantity},
};

use crate::{
    common::parse::{instrument_id_from_lbank_symbol, split_base_quote},
    http::models::LBankAccuracy,
};

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

#[cfg(test)]
mod tests {
    use super::*;

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
