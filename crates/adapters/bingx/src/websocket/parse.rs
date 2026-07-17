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

//! Converts BingX WebSocket messages into Nautilus data events.

use std::str::FromStr;

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        BookOrder, OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API, TradeTick,
        order::OrderId,
    },
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};

use super::messages::{BingXWsDepthData, BingXWsTradeData};

fn parse_price(value: &str, precision: u8) -> anyhow::Result<Price> {
    Ok(Price::new(f64::from_str(value)?, precision))
}

fn parse_quantity(value: &str, precision: u8) -> anyhow::Result<Quantity> {
    Ok(Quantity::new(f64::from_str(value)?, precision))
}

/// Builds a snapshot [`OrderBookDeltas_API`] from a BingX limited-depth push.
///
/// BingX depth streams deliver a full (limited-depth) snapshot each tick, so we emit a `Clear`
/// followed by an `Add` per level, all tagged `F_SNAPSHOT`, with `F_LAST` on the final delta.
///
/// # Errors
///
/// Returns an error if any level price/size cannot be parsed, or the batch is empty.
pub fn parse_depth_snapshot(
    data: &BingXWsDepthData,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas_API> {
    let sequence = data.last_update_id.unwrap_or(0).max(0) as u64;
    let snapshot_flag = RecordFlag::F_SNAPSHOT as u8;

    let mut deltas: Vec<OrderBookDelta> =
        Vec::with_capacity(1 + data.bids.len() + data.asks.len());

    // Clear the book first so stale levels from the previous snapshot are dropped.
    deltas.push(OrderBookDelta::clear(
        instrument_id,
        sequence,
        ts_event,
        ts_init,
    ));

    for bid in &data.bids {
        let price = parse_price(&bid[0], price_precision)?;
        let size = parse_quantity(&bid[1], size_precision)?;
        let order = BookOrder::new(OrderSide::Buy, price, size, OrderId::default());
        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            snapshot_flag,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    for ask in &data.asks {
        let price = parse_price(&ask[0], price_precision)?;
        let size = parse_quantity(&ask[1], size_precision)?;
        let order = BookOrder::new(OrderSide::Sell, price, size, OrderId::default());
        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            snapshot_flag,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    // Mark the final delta as the last message in the snapshot packet.
    if let Some(last) = deltas.last_mut() {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    let deltas = OrderBookDeltas::new_checked(instrument_id, deltas)?;
    Ok(OrderBookDeltas_API::new(deltas))
}

/// Builds a [`TradeTick`] from a BingX trade push.
///
/// # Errors
///
/// Returns an error if the price or quantity cannot be parsed.
pub fn parse_trade_tick(
    data: &BingXWsTradeData,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(&data.p, price_precision)?;
    let size = parse_quantity(&data.q, size_precision)?;
    let aggressor_side = if data.m {
        AggressorSide::Seller
    } else {
        AggressorSide::Buyer
    };
    let trade_id = if data.t.is_empty() {
        TradeId::new(&format!("{}-{}", instrument_id.symbol, data.trade_time))
    } else {
        TradeId::new(&data.t)
    };
    let ts_event = UnixNanos::from((data.trade_time.max(0) as u64) * 1_000_000);

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
    use super::*;

    fn instrument() -> InstrumentId {
        InstrumentId::from("BTC-USDT.BINGX")
    }

    #[test]
    fn parses_depth_snapshot() {
        let data = BingXWsDepthData {
            bids: vec![["83656.98".to_string(), "2.570805".to_string()]],
            asks: vec![["84119.73".to_string(), "0.000011".to_string()]],
            last_update_id: Some(13565694850),
        };
        let deltas = parse_depth_snapshot(
            &data,
            instrument(),
            2,
            6,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        // Clear + 1 bid + 1 ask = 3 deltas.
        assert_eq!(deltas.deltas.len(), 3);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[1].action, BookAction::Add);
        // Last delta carries the F_LAST flag.
        let last = deltas.deltas.last().unwrap();
        assert!(last.flags & RecordFlag::F_LAST as u8 != 0);
    }

    #[test]
    fn parses_trade_tick() {
        let data = BingXWsTradeData {
            p: "29110.19".to_string(),
            q: "0.1868".to_string(),
            t: "57903921".to_string(),
            m: true,
            trade_time: 1690214529386,
            s: "BTC-USDT".to_string(),
        };
        let tick = parse_trade_tick(&data, instrument(), 2, 6, UnixNanos::default()).unwrap();
        assert_eq!(tick.aggressor_side, AggressorSide::Seller);
        assert_eq!(tick.trade_id, TradeId::new("57903921"));
    }
}
