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

//! MEXC symbol <-> [`InstrumentId`] conversions.
//!
//! MEXC spot market ids have **no separator** (e.g. `BTCUSDT`). The instrument's Nautilus symbol
//! is therefore identical to the exchange market id, making the round-trip trivial. Splitting a raw
//! id like `BTCUSDT` into base/quote is NOT reliable from the string alone — the base/quote assets
//! must come from the `exchangeInfo` `baseAsset`/`quoteAsset` fields (see [`crate::http::parse`]).

use nautilus_core::UnixNanos;
use nautilus_model::identifiers::{InstrumentId, Symbol};

use super::consts::MEXC_VENUE;

/// Converts a MEXC market id (e.g. `BTCUSDT`) to a Nautilus [`InstrumentId`] on the MEXC venue.
#[must_use]
pub fn instrument_id_from_symbol(symbol: &str) -> InstrumentId {
    InstrumentId::new(Symbol::from(symbol), *MEXC_VENUE)
}

/// Returns the MEXC market id for a Nautilus [`InstrumentId`] (the symbol string as-is).
#[must_use]
pub fn symbol_from_instrument_id(instrument_id: &InstrumentId) -> String {
    instrument_id.symbol.as_str().to_string()
}

/// Parses a MEXC millisecond timestamp into [`UnixNanos`].
#[must_use]
pub fn parse_millisecond_timestamp(millis: i64) -> UnixNanos {
    UnixNanos::from(millis.max(0) as u64 * 1_000_000)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("BTCUSDT")]
    #[case("ETHUSDC")]
    #[case("MXUSDT")]
    fn test_symbol_instrument_id_roundtrip(#[case] market_id: &str) {
        let id = instrument_id_from_symbol(market_id);
        assert_eq!(id.venue.as_str(), "MEXC");
        assert_eq!(id.symbol.as_str(), market_id);
        // Round-trip back to the exchange market id.
        assert_eq!(symbol_from_instrument_id(&id), market_id);
    }

    #[rstest]
    fn test_parse_millisecond_timestamp() {
        assert_eq!(parse_millisecond_timestamp(1_700_000_000_000).as_u64(), 1_700_000_000_000_000_000);
        assert_eq!(parse_millisecond_timestamp(-5).as_u64(), 0);
    }
}
