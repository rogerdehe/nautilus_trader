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

//! Parse LBank CONTRACT v3 WebSocket frames into Nautilus domain types.

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, TradeTick},
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, TradeId},
};

use crate::{
    http::parse::{parse_price, parse_quantity},
    websocket::contract_messages::{ContractWsFrame, ContractWsTrade},
};

/// Maps the contract trade `direction` code to the taker [`AggressorSide`] (`"0"`=buy, `"1"`=sell).
#[must_use]
pub fn contract_aggressor(direction: Option<&str>) -> AggressorSide {
    match direction {
        Some("0") => AggressorSide::Buyer,
        Some("1") => AggressorSide::Seller,
        _ => AggressorSide::NoAggressor,
    }
}

fn push_side(
    levels: &[[String; 2]],
    side: OrderSide,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    out: &mut Vec<OrderBookDelta>,
) -> anyhow::Result<()> {
    for level in levels {
        let price = parse_price(level[0].as_str(), price_precision)?;
        let size = parse_quantity(level[1].as_str(), size_precision)?;
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

/// Parses a contract depth push (full snapshot: top-level `b`=bids / `s`=asks) into
/// [`OrderBookDeltas`]: a `Clear` (flagged `F_SNAPSHOT`) followed by `Add` deltas for every level
/// (bids=Buy, asks=Sell), the last carrying `F_LAST`.
pub fn parse_contract_depth(
    frame: &ContractWsFrame,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let empty: Vec<[String; 2]> = Vec::new();
    let bids = frame.b.as_ref().unwrap_or(&empty);
    let asks = frame.s.as_ref().unwrap_or(&empty);

    // `w` is the server push time in milliseconds; fall back to receive time.
    let ts_event = frame
        .w
        .filter(|ms| *ms > 0)
        .map_or(ts_init, |ms| UnixNanos::from(ms as u64 * 1_000_000));

    let mut deltas: Vec<OrderBookDelta> = Vec::with_capacity(bids.len() + asks.len() + 1);
    let mut clear = OrderBookDelta::clear(instrument_id, 0, ts_event, ts_init);
    clear.flags |= RecordFlag::F_SNAPSHOT as u8;
    deltas.push(clear);

    push_side(bids, OrderSide::Buy, instrument_id, price_precision, size_precision, ts_event, ts_init, &mut deltas)?;
    push_side(asks, OrderSide::Sell, instrument_id, price_precision, size_precision, ts_event, ts_init, &mut deltas)?;

    if let Some(last) = deltas.last_mut() {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
}

/// Parses a single contract trade payload into a [`TradeTick`]. `c`=price, `b`=volume, `d`=direction,
/// `e`=trade time (SECONDS), `f`=trade id.
pub fn parse_contract_trade(
    trade: &ContractWsTrade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(trade.c.as_str(), price_precision)?;
    let size = parse_quantity(trade.b.as_str(), size_precision)?;
    let aggressor_side = contract_aggressor(trade.d.as_deref());

    // `e` is exchange trade time in SECONDS.
    let ts_event = trade
        .e
        .as_deref()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .map_or(ts_init, |s| UnixNanos::from(s * 1_000_000_000));

    let trade_id = match &trade.f {
        Some(id) if !id.is_empty() => TradeId::new(id),
        _ => TradeId::new(format!("{}-{}", ts_event.as_u64(), trade.c).as_str()),
    };

    TradeTick::new_checked(instrument_id, price, size, aggressor_side, trade_id, ts_event, ts_init)
}

#[cfg(test)]
mod tests {
    use nautilus_model::types::Price;

    use super::*;
    use crate::websocket::contract_messages::ContractWsFrame;

    #[test]
    fn aggressor_mapping() {
        assert_eq!(contract_aggressor(Some("0")), AggressorSide::Buyer);
        assert_eq!(contract_aggressor(Some("1")), AggressorSide::Seller);
        assert_eq!(contract_aggressor(None), AggressorSide::NoAggressor);
    }

    #[test]
    fn depth_snapshot_shape() {
        let raw = r#"{"b":[["63227","20.38"],["63226","6.77"]],"s":[["63229","1.10"]],"w":1785662431000,"x":3,"z":4}"#;
        let frame: ContractWsFrame = serde_json::from_str(raw).unwrap();
        let id = InstrumentId::from("BTCUSDT.LBANK");
        let deltas = parse_contract_depth(&frame, id, 1, 2, UnixNanos::default()).unwrap();
        assert_eq!(deltas.deltas.len(), 4); // clear + 2 bids + 1 ask
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert!(deltas.deltas[0].flags & RecordFlag::F_SNAPSHOT as u8 != 0);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[3].order.side, OrderSide::Sell);
        assert!(deltas.deltas.last().unwrap().flags & RecordFlag::F_LAST as u8 != 0);
        assert_eq!(deltas.deltas[1].ts_event.as_u64(), 1_785_662_431_000_000_000);
    }

    #[test]
    fn trade_shape() {
        let t = ContractWsTrade {
            a: Some("BTCUSDT".into()),
            b: "0.0012".into(),
            c: "63435.1".into(),
            d: Some("1".into()),
            e: Some("1785659150".into()),
            f: Some("1007931922450694".into()),
        };
        let id = InstrumentId::from("BTCUSDT.LBANK");
        let tick = parse_contract_trade(&t, id, 1, 4, UnixNanos::default()).unwrap();
        assert_eq!(tick.aggressor_side, AggressorSide::Seller);
        assert_eq!(tick.price, Price::from("63435.1"));
        assert_eq!(tick.ts_event.as_u64(), 1_785_659_150_000_000_000);
    }
}
