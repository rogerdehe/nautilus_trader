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

//! BingX symbol <-> Nautilus identifier conversion.
//!
//! BingX uses `BASE-QUOTE` (hyphen, uppercase) for BOTH spot and swap markets (`parse_market` in
//! CCXT splits `id` on `-`). The Nautilus mapping keeps the hyphen verbatim so it is REVERSIBLE:
//! `BTC-USDT` <-> `BTC-USDT.BINGX`. No case transform is needed (BingX ids are already uppercase).

use nautilus_model::identifiers::{InstrumentId, Symbol};

use crate::common::consts::bingx_venue;

/// Converts a BingX market id (`BTC-USDT`) to a Nautilus [`InstrumentId`] (`BTC-USDT.BINGX`).
#[must_use]
pub fn instrument_id_from_bingx_symbol(bingx_symbol: &str) -> InstrumentId {
    InstrumentId::new(Symbol::from(bingx_symbol), bingx_venue())
}

/// Converts a Nautilus [`InstrumentId`] back to the BingX market id (`BTC-USDT`).
#[must_use]
pub fn bingx_symbol_from_instrument_id(instrument_id: &InstrumentId) -> String {
    instrument_id.symbol.as_str().to_string()
}

/// Splits a BingX market id into `(base, quote)` uppercased currency codes, or `None` if the id is
/// not a single `BASE-QUOTE` form.
#[must_use]
pub fn split_base_quote(bingx_symbol: &str) -> Option<(String, String)> {
    let mut parts = bingx_symbol.split('-');
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
    #[case("BTC-USDT", "BTC-USDT.BINGX", "BTC", "USDT")]
    #[case("1INCH-USDT", "1INCH-USDT.BINGX", "1INCH", "USDT")]
    #[case("ETH-BTC", "ETH-BTC.BINGX", "ETH", "BTC")]
    fn symbol_roundtrip(
        #[case] bingx: &str,
        #[case] expected_id: &str,
        #[case] base: &str,
        #[case] quote: &str,
    ) {
        let id = instrument_id_from_bingx_symbol(bingx);
        assert_eq!(id.to_string(), expected_id);
        assert_eq!(bingx_symbol_from_instrument_id(&id), bingx);
        assert_eq!(split_base_quote(bingx), Some((base.to_string(), quote.to_string())));
    }

    #[rstest]
    fn split_rejects_non_pair() {
        assert_eq!(split_base_quote("BTCUSDT"), None);
        assert_eq!(split_base_quote("A-B-C"), None);
    }
}
