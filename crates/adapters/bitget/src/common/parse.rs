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

//! Symbol/identifier parsing helpers for the Bitget adapter.
//!
//! Bitget market ids are *concatenated* (`BTCUSDT`, no separator), so — like Binance — base/quote
//! cannot be recovered from the id string alone; they come from the instrument definition. The raw
//! id is used verbatim as the nautilus [`Symbol`] for spot; USDT-margined perpetuals get a `-PERP`
//! suffix to disambiguate from the spot pair of the same id.

use nautilus_model::identifiers::{InstrumentId, Symbol};

use super::{consts::BITGET_VENUE, enums::BitgetProductType};

/// Suffix appended to perpetual instrument symbols to disambiguate from the spot pair.
pub const PERP_SUFFIX: &str = "-PERP";

/// Builds a nautilus [`InstrumentId`] from a raw Bitget market id and its product type.
#[must_use]
pub fn instrument_id_from_raw(raw_symbol: &str, product: BitgetProductType) -> InstrumentId {
    let symbol = if product.is_spot() {
        Symbol::from_str_unchecked(raw_symbol)
    } else {
        Symbol::from_str_unchecked(format!("{raw_symbol}{PERP_SUFFIX}"))
    };
    InstrumentId::new(symbol, *BITGET_VENUE)
}

/// Recovers the raw Bitget market id (and whether it is a perpetual) from a nautilus [`Symbol`].
#[must_use]
pub fn raw_symbol_from_symbol(symbol: &Symbol) -> (String, bool) {
    let s = symbol.as_str();
    match s.strip_suffix(PERP_SUFFIX) {
        Some(raw) => (raw.to_string(), true),
        None => (s.to_string(), false),
    }
}

/// Recovers the raw Bitget market id (and whether it is a perpetual) from an [`InstrumentId`].
#[must_use]
pub fn raw_symbol_from_instrument_id(instrument_id: &InstrumentId) -> (String, bool) {
    raw_symbol_from_symbol(&instrument_id.symbol)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_spot_symbol_roundtrip() {
        let id = instrument_id_from_raw("BTCUSDT", BitgetProductType::Spot);
        assert_eq!(id.symbol.as_str(), "BTCUSDT");
        assert_eq!(id.venue.as_str(), "BITGET");
        let (raw, is_perp) = raw_symbol_from_instrument_id(&id);
        assert_eq!(raw, "BTCUSDT");
        assert!(!is_perp);
    }

    #[rstest]
    fn test_perp_symbol_roundtrip() {
        let id = instrument_id_from_raw("BTCUSDT", BitgetProductType::UsdtFutures);
        assert_eq!(id.symbol.as_str(), "BTCUSDT-PERP");
        let (raw, is_perp) = raw_symbol_from_instrument_id(&id);
        assert_eq!(raw, "BTCUSDT");
        assert!(is_perp);
    }

    #[rstest]
    fn test_perp_distinct_from_spot() {
        let spot = instrument_id_from_raw("ETHUSDT", BitgetProductType::Spot);
        let perp = instrument_id_from_raw("ETHUSDT", BitgetProductType::UsdtFutures);
        assert_ne!(spot, perp);
    }
}
