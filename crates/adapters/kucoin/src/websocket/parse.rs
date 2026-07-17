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

//! Conversions from KuCoin WebSocket payloads to Nautilus data types.
//!
//! The spot data path uses `/spotMarket/level2Depth50`, which pushes a FULL top-50 snapshot in
//! every frame. Each snapshot is emitted as [`OrderBookDeltas`] = `Clear` + `Add` levels with
//! `F_SNAPSHOT`, and `F_LAST` set on the final delta (no REST snapshot / sequence merge needed).

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, TradeTick},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, TradeId},
};

use super::messages::{KuCoinWsDepth, KuCoinWsTrade};
use crate::http::parse::{parse_aggressor_side, parse_price, parse_quantity};

/// Parses a `/spotMarket/level2Depth50` snapshot into [`OrderBookDeltas`].
///
/// # Errors
///
/// Returns an error if any price/size cannot be parsed or [`OrderBookDeltas`] validation fails.
pub fn parse_depth_snapshot(
    depth: &KuCoinWsDepth,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let ts_event = UnixNanos::from((depth.timestamp.max(0) as u64) * 1_000_000);
    let flags = RecordFlag::F_SNAPSHOT as u8;
    let sequence = depth.timestamp.max(0) as u64;

    let mut deltas = Vec::with_capacity(depth.bids.len() + depth.asks.len() + 1);
    // Snapshot begins with a Clear so any stale book state is dropped.
    deltas.push(OrderBookDelta::clear(
        instrument_id,
        sequence,
        ts_event,
        ts_init,
    ));

    for level in &depth.bids {
        let price = parse_price(&level[0], price_precision)?;
        let size = parse_quantity(&level[1], size_precision)?;
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

    for level in &depth.asks {
        let price = parse_price(&level[0], price_precision)?;
        let size = parse_quantity(&level[1], size_precision)?;
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

    // Mark the final delta as the last in the snapshot batch (F_SNAPSHOT | F_LAST).
    if let Some(last) = deltas.last_mut() {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
}

/// Parses a `/market/match` trade push into a Nautilus [`TradeTick`].
///
/// KuCoin `time` is expressed in NANOSECONDS (as a string).
///
/// # Errors
///
/// Returns an error if numeric fields cannot be parsed or [`TradeTick`] validation fails.
pub fn parse_ws_trade(
    trade: &KuCoinWsTrade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(&trade.price, price_precision)?;
    let size = parse_quantity(&trade.size, size_precision)?;
    let aggressor = parse_aggressor_side(&trade.side);
    let trade_id = TradeId::new(trade.trade_id.as_str());
    let ts_event = UnixNanos::from(trade.time.parse::<u64>().unwrap_or(0));

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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::parse::instrument_id_from_kucoin_symbol;

    #[rstest]
    fn depth_snapshot_builds_clear_plus_levels() {
        let depth = KuCoinWsDepth {
            asks: vec![["42815.6".to_string(), "1.24".to_string()]],
            bids: vec![["42815.5".to_string(), "0.08".to_string()]],
            timestamp: 1_707_204_474_018,
        };
        let id = instrument_id_from_kucoin_symbol("BTC-USDT");
        let deltas = parse_depth_snapshot(&depth, id, 1, 8, UnixNanos::default()).unwrap();
        // Clear + 1 bid + 1 ask = 3 deltas.
        assert_eq!(deltas.deltas.len(), 3);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[0].flags, RecordFlag::F_SNAPSHOT as u8);
        let last = deltas.deltas.last().unwrap();
        assert_eq!(
            last.flags,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
        );
    }

    #[rstest]
    fn ws_trade_parses_nanosecond_time() {
        let trade = KuCoinWsTrade {
            symbol: "BTC-USDT".to_string(),
            side: "buy".to_string(),
            size: "0.005".to_string(),
            price: "9345".to_string(),
            time: "1580559434436443257".to_string(),
            trade_id: "5e356c4aeefabd62c62a1ece".to_string(),
            sequence: None,
        };
        let id = instrument_id_from_kucoin_symbol("BTC-USDT");
        let tick = parse_ws_trade(&trade, id, 0, 3, UnixNanos::default()).unwrap();
        assert_eq!(tick.ts_event, UnixNanos::from(1_580_559_434_436_443_257));
        assert_eq!(tick.aggressor_side, nautilus_model::enums::AggressorSide::Buyer);
    }
}
