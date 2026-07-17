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

//! Conversions from Bitget WebSocket push payloads into nautilus data types.

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, TradeTick},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::InstrumentId,
};

use super::messages::{BitgetWsBook, BitgetWsTrade};
use crate::{
    common::enums::BitgetProductType,
    http::parse::{parse_price, parse_quantity},
};

/// Maps a Bitget WebSocket `instType` (case-insensitive) to a [`BitgetProductType`].
#[must_use]
pub fn product_from_inst_type(inst_type: &str) -> BitgetProductType {
    match inst_type.to_ascii_uppercase().as_str() {
        "SPOT" => BitgetProductType::Spot,
        "COIN-FUTURES" => BitgetProductType::CoinFutures,
        "USDC-FUTURES" => BitgetProductType::UsdcFutures,
        _ => BitgetProductType::UsdtFutures,
    }
}

fn millis_str_to_nanos(ms: &str) -> UnixNanos {
    UnixNanos::from(ms.parse::<u64>().unwrap_or(0) * 1_000_000)
}

/// Parses a Bitget depth snapshot into [`OrderBookDeltas`].
///
/// Bitget `books1`/`books5`/`books15` channels push a full snapshot each tick, so this emits a
/// `Clear` followed by `Add` deltas, all flagged `F_SNAPSHOT`, with `F_LAST` on the final delta.
pub fn parse_book_snapshot(
    book: &BitgetWsBook,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let ts_event = millis_str_to_nanos(&book.ts);
    let sequence = 0u64;
    let flags = RecordFlag::F_SNAPSHOT as u8;

    let mut deltas = Vec::with_capacity(book.bids.len() + book.asks.len() + 1);
    deltas.push(OrderBookDelta::clear(
        instrument_id,
        sequence,
        ts_event,
        ts_init,
    ));

    for bid in &book.bids {
        let price = parse_price(&bid[0], price_precision)?;
        let size = parse_quantity(&bid[1], size_precision)?;
        let order = BookOrder::new(OrderSide::Buy, price, size, 0);
        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    for ask in &book.asks {
        let price = parse_price(&ask[0], price_precision)?;
        let size = parse_quantity(&ask[1], size_precision)?;
        let order = BookOrder::new(OrderSide::Sell, price, size, 0);
        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    if let Some(last) = deltas.last_mut() {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    OrderBookDeltas::new_checked(instrument_id, deltas).map_err(|e| anyhow::anyhow!(e.to_string()))
}

/// Parses a Bitget public trade payload into a [`TradeTick`].
pub fn parse_ws_trade(
    trade: &BitgetWsTrade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    use nautilus_model::{enums::AggressorSide, identifiers::TradeId};

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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::parse::instrument_id_from_raw;

    #[rstest]
    fn test_parse_book_snapshot() {
        // Shape lifted from CCXT `handle_order_book` spot docstring.
        let book = BitgetWsBook {
            asks: vec![
                ["21041.11".to_string(), "0.0445".to_string()],
                ["21041.16".to_string(), "0.0411".to_string()],
            ],
            bids: vec![["21040.76".to_string(), "0.0417".to_string()]],
            ts: "1656413855484".to_string(),
        };
        let id = instrument_id_from_raw("BTCUSDT", BitgetProductType::Spot);
        let deltas = parse_book_snapshot(&book, id, 2, 4, UnixNanos::default()).unwrap();
        // 1 clear + 1 bid + 2 asks = 4 deltas.
        assert_eq!(deltas.deltas.len(), 4);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        // `OrderBookDelta::clear` sets F_SNAPSHOT itself.
        assert_eq!(
            deltas.deltas[0].flags & RecordFlag::F_SNAPSHOT as u8,
            RecordFlag::F_SNAPSHOT as u8
        );
        let last = deltas.deltas.last().unwrap();
        assert_eq!(last.flags & RecordFlag::F_LAST as u8, RecordFlag::F_LAST as u8);
        assert_eq!(
            last.flags & RecordFlag::F_SNAPSHOT as u8,
            RecordFlag::F_SNAPSHOT as u8
        );
    }

    #[rstest]
    fn test_parse_ws_trade() {
        // Shape lifted from CCXT `handle_trades` spot docstring.
        let trade = BitgetWsTrade {
            ts: "1701910980366".to_string(),
            price: "43854.01".to_string(),
            size: "0.0535".to_string(),
            side: "buy".to_string(),
            trade_id: "1116461060594286593".to_string(),
        };
        let id = instrument_id_from_raw("BTCUSDT", BitgetProductType::Spot);
        let tick = parse_ws_trade(&trade, id, 2, 4, UnixNanos::default()).unwrap();
        assert_eq!(tick.price.to_string(), "43854.01");
        assert_eq!(
            tick.aggressor_side,
            nautilus_model::enums::AggressorSide::Buyer
        );
    }

    #[rstest]
    fn test_product_from_inst_type() {
        assert_eq!(product_from_inst_type("SPOT"), BitgetProductType::Spot);
        assert_eq!(product_from_inst_type("spot"), BitgetProductType::Spot);
        assert_eq!(
            product_from_inst_type("USDT-FUTURES"),
            BitgetProductType::UsdtFutures
        );
    }
}
