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

//! HashKey symbol <-> Nautilus identifier conversion.
//!
//! HashKey market ids have NO separator for spot (`BTCUSDT`) and a `-PERPETUAL` suffix for swap
//! (`BTCUSDT-PERPETUAL`). Because a spot id like `BTCUSDT` cannot be split into base/quote by string
//! alone, the mapping to a Nautilus [`InstrumentId`] is a lossless IDENTITY (uppercased): the raw
//! exchange id is carried verbatim as the symbol, e.g. `BTCUSDT` <-> `BTCUSDT.HASHKEY`. Base/quote
//! for building instruments come from the `exchangeInfo` `baseAsset`/`quoteAsset` fields, never from
//! parsing this string.

use nautilus_model::identifiers::{InstrumentId, Symbol};

use crate::common::consts::{PERPETUAL_SUFFIX, hashkey_venue};

/// Converts a HashKey market id (`BTCUSDT`) to a Nautilus [`InstrumentId`] (`BTCUSDT.HASHKEY`).
#[must_use]
pub fn instrument_id_from_hashkey_symbol(hashkey_symbol: &str) -> InstrumentId {
    InstrumentId::new(
        Symbol::from(hashkey_symbol.to_uppercase().as_str()),
        hashkey_venue(),
    )
}

/// Converts a Nautilus [`InstrumentId`] back to the HashKey market id (`BTCUSDT`).
#[must_use]
pub fn hashkey_symbol_from_instrument_id(instrument_id: &InstrumentId) -> String {
    instrument_id.symbol.as_str().to_string()
}

/// Returns `true` when the market id denotes a HashKey perpetual swap (`*-PERPETUAL`).
#[must_use]
pub fn is_swap_symbol(hashkey_symbol: &str) -> bool {
    hashkey_symbol.ends_with(PERPETUAL_SUFFIX)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("BTCUSDT", "BTCUSDT.HASHKEY")]
    #[case("ETHUSDT", "ETHUSDT.HASHKEY")]
    #[case("BTCUSDT-PERPETUAL", "BTCUSDT-PERPETUAL.HASHKEY")]
    fn symbol_roundtrip(#[case] hashkey: &str, #[case] expected_id: &str) {
        let id = instrument_id_from_hashkey_symbol(hashkey);
        assert_eq!(id.to_string(), expected_id);
        assert_eq!(hashkey_symbol_from_instrument_id(&id), hashkey);
    }

    #[rstest]
    fn detects_swap() {
        assert!(is_swap_symbol("BTCUSDT-PERPETUAL"));
        assert!(!is_swap_symbol("BTCUSDT"));
    }
}
