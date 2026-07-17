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

//! Converts inbound LBank WebSocket frames into Nautilus data.

use chrono::NaiveDateTime;
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::TradeTick,
    enums::AggressorSide,
    identifiers::{InstrumentId, TradeId},
    instruments::{Instrument, InstrumentAny},
};

use crate::{
    common::parse::instrument_id_from_lbank_symbol,
    http::{
        models::{LBankDepth, LBankLevel},
        parse::{aggressor_from_direction, parse_depth_snapshot, parse_price, parse_quantity},
    },
    websocket::messages::{LbankWsMessage, WsEnvelope, WsTrade},
};

/// Resolves an instrument (for precisions) from its id.
type InstrumentLookup<'a> = &'a dyn Fn(&InstrumentId) -> Option<InstrumentAny>;

/// Parses an LBank WS ISO-8601 timestamp (`"2019-06-28T17:49:22.722"`, no timezone) as UTC nanos.
#[must_use]
pub fn parse_ws_ts(ts: &str, fallback: UnixNanos) -> UnixNanos {
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(ts, fmt)
            && let Some(nanos) = ndt.and_utc().timestamp_nanos_opt()
            && nanos >= 0
        {
            return UnixNanos::from(nanos as u64);
        }
    }
    fallback
}

/// Parses one inbound LBank WS text frame into zero or more [`LbankWsMessage`]s.
///
/// - The app ping (`{"action":"ping","ping":"<uuid>"}`) yields a [`LbankWsMessage::Ping`] carrying
///   the uuid to echo back (MANDATORY keepalive — the read loop replies with a pong).
/// - A `depth` push yields one snapshot [`LbankWsMessage::Deltas`]; a `trade` push yields one
///   [`LbankWsMessage::Trade`].
/// - Subscribe acks / other frames yield an empty vec. Returns `Err` on an error frame or a data
///   frame that cannot be decoded.
pub fn parse_ws_message(
    text: &str,
    lookup: InstrumentLookup<'_>,
) -> anyhow::Result<Vec<LbankWsMessage>> {
    let envelope: WsEnvelope = serde_json::from_str(text)?;

    if envelope.status.as_deref() == Some("error") {
        anyhow::bail!(
            "LBank WS error: {}",
            envelope.message.as_deref().unwrap_or("unknown")
        );
    }

    // CCXT: `type = safe_string_2(message, 'type', 'action')`.
    let msg_type = envelope
        .push_type
        .as_deref()
        .or(envelope.action.as_deref())
        .unwrap_or("");

    if msg_type == "ping" {
        let Some(ping) = envelope.ping.clone() else {
            return Ok(Vec::new());
        };
        return Ok(vec![LbankWsMessage::Ping(ping)]);
    }

    let ts_init = get_atomic_clock_realtime().get_time_ns();
    let ts_event = envelope
        .ts
        .as_deref()
        .map_or(ts_init, |s| parse_ws_ts(s, ts_init));

    match msg_type {
        "depth" => {
            let Some(pair) = envelope.pair.as_deref() else {
                anyhow::bail!("LBank depth push missing pair");
            };
            let instrument_id = instrument_id_from_lbank_symbol(pair);
            let instrument = lookup(&instrument_id)
                .ok_or_else(|| anyhow::anyhow!("no cached instrument for {instrument_id}"))?;
            // The snapshot lives under `depth`, or at the top level for the "request" push shape.
            let depth = match &envelope.depth {
                Some(d) => LBankDepth {
                    asks: clone_levels(&d.asks),
                    bids: clone_levels(&d.bids),
                },
                None => LBankDepth {
                    asks: clone_levels(&envelope.asks),
                    bids: clone_levels(&envelope.bids),
                },
            };
            let deltas = parse_depth_snapshot(
                &depth,
                instrument_id,
                instrument.price_precision(),
                instrument.size_precision(),
                ts_event,
                ts_init,
            )?;
            Ok(vec![LbankWsMessage::Deltas(deltas)])
        }
        "trade" => {
            let Some(pair) = envelope.pair.as_deref() else {
                anyhow::bail!("LBank trade push missing pair");
            };
            let Some(trade) = &envelope.trade else {
                return Ok(Vec::new());
            };
            let instrument_id = instrument_id_from_lbank_symbol(pair);
            let instrument = lookup(&instrument_id)
                .ok_or_else(|| anyhow::anyhow!("no cached instrument for {instrument_id}"))?;
            let tick = parse_ws_trade(
                trade,
                instrument_id,
                instrument.price_precision(),
                instrument.size_precision(),
                ts_init,
            )?;
            Ok(vec![LbankWsMessage::Trade(tick)])
        }
        _ => Ok(Vec::new()),
    }
}

fn clone_levels(levels: &[LBankLevel]) -> Vec<LBankLevel> {
    levels
        .iter()
        .map(|l| LBankLevel(l.0.clone(), l.1.clone()))
        .collect()
}

/// Parses a WS `trade` payload into a [`TradeTick`]. Aggressor from `direction` (maker-flipped);
/// the push carries no trade id so one is synthesized from the timestamp + price + volume.
fn parse_ws_trade(
    trade: &WsTrade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(trade.price.as_str(), price_precision)?;
    let size = parse_quantity(trade.volume.as_str(), size_precision)?;
    let aggressor_side = trade
        .direction
        .as_deref()
        .map_or(AggressorSide::NoAggressor, aggressor_from_direction);
    let ts_event = trade
        .ts
        .as_deref()
        .map_or(ts_init, |s| parse_ws_ts(s, ts_init));
    // LBank spot WS trades carry no native trade id, so synthesize a stable one. It MUST stay within
    // nautilus `TradeId`'s 36-char stack-string limit — the old `{ts}-{price}-{volume}` form reached
    // 42 chars and PANICKED nautilus core (Condition: String exceeds maximum length of 36). Hash the
    // price+volume into 8 hex chars: `{ts_nanos}` (≤20) + `-` + 8 = ≤29 chars, still uniquely keyed
    // by (time, price, volume) for dedup.
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    trade.price.as_str().hash(&mut hasher);
    trade.volume.as_str().hash(&mut hasher);
    let trade_id = TradeId::new(
        format!("{}-{:08x}", ts_event.as_u64(), hasher.finish() as u32).as_str(),
    );
    TradeTick::new_checked(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::BookAction,
        instruments::CurrencyPair,
        types::{Currency, Price, Quantity},
    };

    use super::*;

    fn test_instrument() -> InstrumentAny {
        let id = instrument_id_from_lbank_symbol("btc_usdt");
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
    fn parses_ping() {
        let lookup = |_: &InstrumentId| None;
        let text = r#"{"action":"ping","ping":"a13a939c-5f25-4e06-9981-93cb3b890707"}"#;
        let msgs = parse_ws_message(text, &lookup).unwrap();
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            LbankWsMessage::Ping(id) => assert_eq!(id, "a13a939c-5f25-4e06-9981-93cb3b890707"),
            other => panic!("expected ping, got {other:?}"),
        }
    }

    #[test]
    fn parses_depth_snapshot() {
        let inst = test_instrument();
        let lookup = |id: &InstrumentId| (*id == inst.id()).then(|| inst.clone());
        // Numbers (not strings) as LBank sends over WS.
        let text = r#"{"depth":{"asks":[[42585.84,1.4422]],"bids":[[42585.83,1.8054]]},"count":100,"type":"depth","pair":"btc_usdt","SERVER":"V2","TS":"2024-01-16T08:26:00.413"}"#;
        let mut msgs = parse_ws_message(text, &lookup).unwrap();
        assert_eq!(msgs.len(), 1);
        match msgs.remove(0) {
            LbankWsMessage::Deltas(d) => {
                assert_eq!(d.deltas.len(), 3); // clear + bid + ask
                assert_eq!(d.deltas[0].action, BookAction::Clear);
                // 2024-01-16T08:26:00.413 UTC
                assert_eq!(d.ts_event.as_u64(), 1_705_393_560_413_000_000);
            }
            other => panic!("expected deltas, got {other:?}"),
        }
    }

    #[test]
    fn parses_top_level_depth() {
        let inst = test_instrument();
        let lookup = |id: &InstrumentId| (*id == inst.id()).then(|| inst.clone());
        let text = r#"{"SERVER":"V2","asks":[[42585.84,1.4422]],"bids":[[42585.83,1.8054]],"count":100,"type":"depth","pair":"btc_usdt","TS":"2024-01-16T08:26:00.413"}"#;
        let msgs = parse_ws_message(text, &lookup).unwrap();
        assert_eq!(msgs.len(), 1);
        assert!(matches!(msgs[0], LbankWsMessage::Deltas(_)));
    }

    #[test]
    fn parses_trade() {
        let inst = test_instrument();
        let lookup = |id: &InstrumentId| (*id == inst.id()).then(|| inst.clone());
        let text = r#"{"trade":{"volume":6.3607,"amount":77148.9303,"price":12129,"direction":"sell","TS":"2019-06-28T19:55:49.460"},"type":"trade","pair":"btc_usdt","SERVER":"V2","TS":"2019-06-28T19:55:49.466"}"#;
        let msgs = parse_ws_message(text, &lookup).unwrap();
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            LbankWsMessage::Trade(t) => {
                assert_eq!(t.aggressor_side, AggressorSide::Seller);
                assert_eq!(t.price, Price::from("12129.00"));
                assert_eq!(t.ts_event.as_u64(), 1_561_751_749_460_000_000);
                // Regression: synthesized trade_id MUST fit nautilus TradeId's 36-char stack limit
                // (the old `ts-price-volume` form was 42 chars and panicked nautilus core).
                assert!(t.trade_id.to_string().len() <= 36, "trade_id too long: {}", t.trade_id);
            }
            other => panic!("expected trade, got {other:?}"),
        }
    }

    #[test]
    fn error_frame_bails() {
        let lookup = |_: &InstrumentId| None;
        let text = r#"{"SERVER":"V2","message":"Missing parameter ['kbar']","status":"error","TS":"2024-01-16T08:09:43.314"}"#;
        assert!(parse_ws_message(text, &lookup).is_err());
    }

    #[test]
    fn subscribe_ack_is_empty() {
        let lookup = |_: &InstrumentId| None;
        let text = r#"{"result":true,"SERVER":"V2","action":"subscribe","pair":"btc_usdt"}"#;
        assert!(parse_ws_message(text, &lookup).unwrap().is_empty());
    }
}
