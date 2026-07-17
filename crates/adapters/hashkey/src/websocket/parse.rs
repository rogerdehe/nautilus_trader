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

//! Conversions from HashKey WebSocket messages to Nautilus data types.

use std::str::FromStr;

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, TradeTick},
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};

use crate::websocket::messages::{HashKeyWsDepth, HashKeyWsTrade};

fn ms_to_nanos(ms: i64) -> UnixNanos {
    UnixNanos::from((ms.max(0) as u64) * 1_000_000)
}

fn parse_px(value: &str, precision: u8) -> anyhow::Result<Price> {
    let raw = Price::from_str(value).map_err(|e| anyhow::anyhow!("invalid price '{value}': {e}"))?;
    Ok(Price::new(raw.as_f64(), precision))
}

fn parse_qty(value: &str, precision: u8) -> anyhow::Result<Quantity> {
    let raw =
        Quantity::from_str(value).map_err(|e| anyhow::anyhow!("invalid size '{value}': {e}"))?;
    Ok(Quantity::new(raw.as_f64(), precision))
}

/// Parses a HashKey depth push (a FULL snapshot) into Nautilus [`OrderBookDeltas`].
///
/// Each depth message is a complete snapshot (CCXT `handle_order_book` does `orderbook.reset`), so
/// we emit a `Clear` followed by `Add` deltas for every level, all flagged `F_SNAPSHOT`, and mark
/// the final delta `F_LAST`.
///
/// # Errors
///
/// Returns an error if any level fails to parse or the snapshot is empty.
pub fn parse_ws_depth(
    depth: &HashKeyWsDepth,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let ts_event = ms_to_nanos(depth.t);
    let snap_flag = RecordFlag::F_SNAPSHOT as u8;

    let mut deltas: Vec<OrderBookDelta> =
        Vec::with_capacity(depth.b.len() + depth.a.len() + 1);

    // Clear first — resets any prior book state (matches CCXT `reset`).
    deltas.push(OrderBookDelta::clear(instrument_id, 0, ts_event, ts_init));

    for [px, qty] in &depth.b {
        let price = parse_px(px, price_precision)?;
        let size = parse_qty(qty, size_precision)?;
        let order = BookOrder::new(OrderSide::Buy, price, size, 0);
        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            snap_flag,
            0,
            ts_event,
            ts_init,
        ));
    }

    for [px, qty] in &depth.a {
        let price = parse_px(px, price_precision)?;
        let size = parse_qty(qty, size_precision)?;
        let order = BookOrder::new(OrderSide::Sell, price, size, 0);
        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            snap_flag,
            0,
            ts_event,
            ts_init,
        ));
    }

    // Mark the final delta F_LAST so consumers know the snapshot is complete.
    if let Some(last) = deltas.last_mut() {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    OrderBookDeltas::new_checked(instrument_id, deltas).map_err(Into::into)
}

/// Parses a HashKey trade push entry into a Nautilus [`TradeTick`].
///
/// `m` (`isBuyerMaker`) flags the aggressor: `true` => aggressor SELL, `false` => aggressor BUY
/// (matches CCXT `parse_ws_trade` for public trades).
///
/// # Errors
///
/// Returns an error if the price or size cannot be parsed.
pub fn parse_ws_trade(
    trade: &HashKeyWsTrade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_px(&trade.p, price_precision)?;
    let size = parse_qty(&trade.q, size_precision)?;
    let aggressor_side = if trade.m {
        AggressorSide::Seller
    } else {
        AggressorSide::Buyer
    };
    let id = if trade.v.is_empty() {
        format!("{}-{}", trade.t, trade.p)
    } else {
        trade.v.clone()
    };
    let trade_id = TradeId::new(&id);
    let ts_event = ms_to_nanos(trade.t);

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
    use super::*;
    use crate::common::parse::instrument_id_from_hashkey_symbol;

    #[test]
    fn parses_depth_snapshot() {
        let depth = HashKeyWsDepth {
            t: 1_722_873_144_371,
            b: vec![
                ["1650.5".to_string(), "0.0864".to_string()],
                ["1650.4".to_string(), "1.0".to_string()],
            ],
            a: vec![["1651.0".to_string(), "0.0074".to_string()]],
        };
        let id = instrument_id_from_hashkey_symbol("ETHUSDT");
        let deltas = parse_ws_depth(&depth, id, 2, 4, UnixNanos::default()).unwrap();

        // Clear + 2 bids + 1 ask = 4 deltas.
        assert_eq!(deltas.deltas.len(), 4);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        // Aggregate flags carry F_SNAPSHOT; last delta additionally carries F_LAST.
        assert_eq!(
            deltas.flags & RecordFlag::F_SNAPSHOT as u8,
            RecordFlag::F_SNAPSHOT as u8
        );
        assert_eq!(
            deltas.flags & RecordFlag::F_LAST as u8,
            RecordFlag::F_LAST as u8
        );
    }

    #[test]
    fn parses_ws_trade() {
        let trade = HashKeyWsTrade {
            v: "1745922896272048129".to_string(),
            t: 1_722_866_228_075,
            p: "2340.41".to_string(),
            q: "0.0132".to_string(),
            m: true,
        };
        let id = instrument_id_from_hashkey_symbol("ETHUSDT");
        let tick = parse_ws_trade(&trade, id, 2, 4, UnixNanos::default()).unwrap();
        assert_eq!(tick.aggressor_side, AggressorSide::Seller);
        assert_eq!(tick.trade_id, TradeId::new("1745922896272048129"));
    }
}
