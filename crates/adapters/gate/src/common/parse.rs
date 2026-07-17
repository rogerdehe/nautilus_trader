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

//! Gate symbol <-> Nautilus identifier conversion.
//!
//! Gate market ids are uppercase `BASE_QUOTE` with an underscore (`BTC_USDT`). The underscore is
//! preserved (and already uppercase) so the mapping is REVERSIBLE and unambiguous (collapsing to
//! `BTCUSDT` would be lossy for ids like `1INCH_USDT`).
//!
//! Gate's USDT-settled perpetual contract shares the SAME id as the spot pair (`BTC_USDT`), so the
//! bare `BTC_USDT.GATE` id is reserved for SPOT. Perpetuals are disambiguated with a `-PERP` suffix
//! on the Nautilus symbol (`BTC_USDT-PERP.GATE`) which round-trips back to the Gate contract id.

use nautilus_model::identifiers::{InstrumentId, Symbol};

use crate::common::consts::gate_venue;

/// Suffix appended to the Nautilus symbol for USDT-settled perpetual contracts.
pub const PERP_SUFFIX: &str = "-PERP";

/// Converts a Gate SPOT pair (`BTC_USDT`) to a Nautilus [`InstrumentId`] (`BTC_USDT.GATE`).
#[must_use]
pub fn instrument_id_from_spot_symbol(gate_symbol: &str) -> InstrumentId {
    InstrumentId::new(Symbol::from(gate_symbol.to_uppercase().as_str()), gate_venue())
}

/// Converts a Nautilus SPOT [`InstrumentId`] back to the Gate pair string (`BTC_USDT`).
#[must_use]
pub fn spot_symbol_from_instrument_id(instrument_id: &InstrumentId) -> String {
    instrument_id.symbol.as_str().to_uppercase()
}

/// Converts a Gate USDT perpetual contract (`BTC_USDT`) to a Nautilus [`InstrumentId`]
/// (`BTC_USDT-PERP.GATE`).
#[must_use]
pub fn instrument_id_from_perp_symbol(gate_symbol: &str) -> InstrumentId {
    let sym = format!("{}{PERP_SUFFIX}", gate_symbol.to_uppercase());
    InstrumentId::new(Symbol::from(sym.as_str()), gate_venue())
}

/// Converts a Nautilus [`InstrumentId`] back to the Gate contract/pair id, stripping the `-PERP`
/// suffix when present. Works for both spot and perpetual ids.
#[must_use]
pub fn gate_symbol_from_instrument_id(instrument_id: &InstrumentId) -> String {
    let s = instrument_id.symbol.as_str().to_uppercase();
    s.strip_suffix(PERP_SUFFIX).unwrap_or(&s).to_string()
}

/// Returns `true` when the Nautilus id denotes a Gate perpetual contract (`-PERP` suffix).
#[must_use]
pub fn is_perp(instrument_id: &InstrumentId) -> bool {
    instrument_id.symbol.as_str().to_uppercase().ends_with(PERP_SUFFIX)
}

/// Splits a Gate pair into `(base, quote)` uppercased currency codes, or `None` if the id is not a
/// single `BASE_QUOTE` form.
#[must_use]
pub fn split_base_quote(gate_symbol: &str) -> Option<(String, String)> {
    let mut parts = gate_symbol.split('_');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(base), Some(quote), None) if !base.is_empty() && !quote.is_empty() => {
            Some((base.to_uppercase(), quote.to_uppercase()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("BTC_USDT", "BTC_USDT.GATE", "BTC", "USDT")]
    #[case("1INCH_USDT", "1INCH_USDT.GATE", "1INCH", "USDT")]
    #[case("ETH_BTC", "ETH_BTC.GATE", "ETH", "BTC")]
    fn spot_symbol_roundtrip(
        #[case] gate: &str,
        #[case] expected_id: &str,
        #[case] base: &str,
        #[case] quote: &str,
    ) {
        let id = instrument_id_from_spot_symbol(gate);
        assert_eq!(id.to_string(), expected_id);
        assert_eq!(spot_symbol_from_instrument_id(&id), gate);
        assert_eq!(gate_symbol_from_instrument_id(&id), gate);
        assert!(!is_perp(&id));
        assert_eq!(split_base_quote(gate), Some((base.to_string(), quote.to_string())));
    }

    #[rstest]
    #[case("BTC_USDT", "BTC_USDT-PERP.GATE")]
    #[case("ETH_USDT", "ETH_USDT-PERP.GATE")]
    fn perp_symbol_roundtrip(#[case] gate: &str, #[case] expected_id: &str) {
        let id = instrument_id_from_perp_symbol(gate);
        assert_eq!(id.to_string(), expected_id);
        assert!(is_perp(&id));
        assert_eq!(gate_symbol_from_instrument_id(&id), gate);
    }

    #[rstest]
    fn split_rejects_non_pair() {
        assert_eq!(split_base_quote("BTCUSDT"), None);
        assert_eq!(split_base_quote("A_B_C"), None);
    }
}
