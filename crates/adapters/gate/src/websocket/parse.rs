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

//! Converts inbound Gate WebSocket messages into Nautilus data.

use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::OrderBookDeltas,
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};

use crate::{
    common::{
        consts::{CH_SPOT_ORDER_BOOK, CH_SPOT_TRADES},
        parse::instrument_id_from_spot_symbol,
    },
    http::{
        models::{SpotBookLevel, SpotOrderBook, SpotTrade},
        parse::{parse_order_book_snapshot, parse_trade_tick},
    },
    websocket::messages::{GateWsMessage, WsEnvelope, WsSpotOrderBook},
};

/// Looks up precisions for a Gate market id from the instrument cache.
type InstrumentLookup<'a> = &'a dyn Fn(&InstrumentId) -> Option<InstrumentAny>;

fn ts_from_ms(t: Option<i64>, fallback: UnixNanos) -> UnixNanos {
    match t {
        Some(ms) if ms > 0 => UnixNanos::from(ms as u64 * 1_000_000),
        _ => fallback,
    }
}

/// A `spot.trades` result: Gate may push either a single trade object or an array of them
/// (CCXT `handle_trades` normalizes a non-list into a list before parsing).
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum SpotTradeResult {
    Many(Vec<SpotTrade>),
    One(SpotTrade),
}

impl SpotTradeResult {
    fn into_vec(self) -> Vec<SpotTrade> {
        match self {
            Self::Many(v) => v,
            Self::One(t) => vec![t],
        }
    }
}

/// Parses one inbound Gate WS text frame into zero or more [`GateWsMessage`]s.
///
/// Returns an empty vec for frames that carry no market data (subscribe acks, pongs, unknown
/// channels). A book snapshot yields one message; a `spot.trades` push yields one per trade.
/// Returns `Err` only when a data frame cannot be decoded.
pub fn parse_ws_message(
    text: &str,
    lookup: InstrumentLookup<'_>,
) -> anyhow::Result<Vec<GateWsMessage>> {
    let envelope: WsEnvelope = serde_json::from_str(text)?;

    if let Some(err) = &envelope.error
        && (err.code.is_some() || err.message.is_some())
    {
        anyhow::bail!(
            "Gate WS error on channel '{}': code={:?} message={:?}",
            envelope.channel,
            err.code,
            err.message
        );
    }

    // Only `update`/`all` events carry data; ignore `subscribe`/`unsubscribe` acks.
    let event = envelope.event.as_deref().unwrap_or("");
    if matches!(event, "subscribe" | "unsubscribe") {
        return Ok(Vec::new());
    }

    let ts_init = get_atomic_clock_realtime().get_time_ns();

    match envelope.channel.as_str() {
        CH_SPOT_ORDER_BOOK => {
            let book: WsSpotOrderBook = serde_json::from_value(envelope.result)?;
            let instrument_id = instrument_id_from_spot_symbol(&book.s);
            let instrument = lookup(&instrument_id)
                .ok_or_else(|| anyhow::anyhow!("no cached instrument for {instrument_id}"))?;
            let ts_event = ts_from_ms(book.t, ts_init);
            let deltas = build_book_deltas(&book, &instrument, ts_event, ts_init)?;
            Ok(vec![GateWsMessage::Deltas(deltas)])
        }
        CH_SPOT_TRADES => {
            let trades: SpotTradeResult = serde_json::from_value(envelope.result)?;
            let mut out = Vec::new();
            for trade in trades.into_vec() {
                let Some(market_id) = trade.currency_pair.clone() else {
                    log::debug!("Gate WS trade missing currency_pair; skipping");
                    continue;
                };
                let instrument_id = instrument_id_from_spot_symbol(&market_id);
                let Some(instrument) = lookup(&instrument_id) else {
                    log::debug!("no cached instrument for {instrument_id}; skipping trade");
                    continue;
                };
                let tick = parse_trade_tick(
                    &trade,
                    instrument_id,
                    instrument.price_precision(),
                    instrument.size_precision(),
                    ts_init,
                )?;
                out.push(GateWsMessage::Trade(tick));
            }
            Ok(out)
        }
        _ => Ok(Vec::new()),
    }
}

fn build_book_deltas(
    book: &WsSpotOrderBook,
    instrument: &InstrumentAny,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    // Reuse the REST snapshot builder by mapping the WS shape onto `SpotOrderBook`.
    let rest = SpotOrderBook {
        id: None,
        current: book.t,
        update: book.t,
        asks: book
            .asks
            .iter()
            .map(|l| SpotBookLevel(l.0.clone(), l.1.clone()))
            .collect(),
        bids: book
            .bids
            .iter()
            .map(|l| SpotBookLevel(l.0.clone(), l.1.clone()))
            .collect(),
    };
    parse_order_book_snapshot(
        &rest,
        instrument.id(),
        instrument.price_precision(),
        instrument.size_precision(),
        ts_event,
        ts_init,
    )
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::BookAction,
        instruments::{CurrencyPair, InstrumentAny},
        types::{Currency, Price, Quantity},
    };

    use super::*;

    fn test_instrument() -> InstrumentAny {
        let id = instrument_id_from_spot_symbol("BTC_USDT");
        InstrumentAny::CurrencyPair(CurrencyPair::new(
            id,
            id.symbol,
            Currency::get_or_create_crypto("BTC"),
            Currency::get_or_create_crypto("USDT"),
            2,
            4,
            Price::from("0.01"),
            Quantity::from("0.0001"),
            None, None, None, None, None, None, None, None, None, None, None, None, None, None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    #[test]
    fn parses_order_book_snapshot() {
        let inst = test_instrument();
        let lookup = |id: &InstrumentId| {
            if *id == inst.id() { Some(inst.clone()) } else { None }
        };
        let text = r#"{"time":1650189272,"channel":"spot.order_book","event":"update","result":{"t":1650189272515,"lastUpdateId":1,"s":"BTC_USDT","bids":[["19080.24","0.1"]],"asks":[["19080.25","0.2"]]}}"#;
        let mut msgs = parse_ws_message(text, &lookup).unwrap();
        assert_eq!(msgs.len(), 1);
        match msgs.remove(0) {
            GateWsMessage::Deltas(d) => {
                assert_eq!(d.deltas.len(), 3); // clear + bid + ask
                assert_eq!(d.deltas[0].action, BookAction::Clear);
                assert_eq!(d.ts_event.as_u64(), 1_650_189_272_515_000_000);
            }
            other => panic!("expected deltas, got {other:?}"),
        }
    }

    #[test]
    fn parses_trade() {
        let inst = test_instrument();
        let lookup = |id: &InstrumentId| {
            if *id == inst.id() { Some(inst.clone()) } else { None }
        };
        let text = r#"{"time":1648725035,"channel":"spot.trades","event":"update","result":{"id":"3130257995","create_time":"1648725035","create_time_ms":"1648725035923.0","side":"sell","currency_pair":"BTC_USDT","amount":"0.0116","price":"130.11"}}"#;
        let msgs = parse_ws_message(text, &lookup).unwrap();
        assert_eq!(msgs.len(), 1);
        assert!(matches!(msgs[0], GateWsMessage::Trade(_)));
    }

    #[test]
    fn parses_trades_array() {
        let inst = test_instrument();
        let lookup = |id: &InstrumentId| {
            if *id == inst.id() { Some(inst.clone()) } else { None }
        };
        // Gate can push `result` as an array (CCXT `handle_trades` normalizes single -> list).
        let text = r#"{"time":1648725035,"channel":"spot.trades","event":"update","result":[{"id":"1","create_time_ms":"1648725035923.0","side":"buy","currency_pair":"BTC_USDT","amount":"0.01","price":"130.10"},{"id":"2","create_time_ms":"1648725035999.0","side":"sell","currency_pair":"BTC_USDT","amount":"0.02","price":"130.12"}]}"#;
        let msgs = parse_ws_message(text, &lookup).unwrap();
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn ignores_subscribe_ack() {
        let lookup = |_: &InstrumentId| None;
        let text = r#"{"time":1649062304,"channel":"spot.order_book","event":"subscribe","result":{"status":"success"}}"#;
        assert!(parse_ws_message(text, &lookup).unwrap().is_empty());
    }
}
