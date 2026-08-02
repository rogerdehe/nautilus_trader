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

//! LBank symbol <-> Nautilus identifier conversion.
//!
//! LBank spot pairs are lowercase `base_quote` with an underscore (`btc_usdt`). The mapping to Nautilus
//! keeps the underscore (uppercased) so it is REVERSIBLE and unambiguous — collapsing to `BTCUSDT`
//! would be lossy for pairs like `1inch_usdt`. So `btc_usdt` <-> `BTC_USDT.LBANK`.

use nautilus_model::identifiers::{InstrumentId, Symbol};

use crate::common::consts::lbank_venue;

/// Converts an LBank pair (`btc_usdt`) to a Nautilus [`InstrumentId`] (`BTC_USDT.LBANK`).
#[must_use]
pub fn instrument_id_from_lbank_symbol(lbank_symbol: &str) -> InstrumentId {
    InstrumentId::new(Symbol::from(lbank_symbol.to_uppercase().as_str()), lbank_venue())
}

/// Converts a Nautilus [`InstrumentId`] back to the LBank pair string (`btc_usdt`).
#[must_use]
pub fn lbank_symbol_from_instrument_id(instrument_id: &InstrumentId) -> String {
    instrument_id.symbol.as_str().to_lowercase()
}

/// Splits an LBank pair into `(base, quote)` uppercased currency codes, or `None` if the pair is not a
/// single `base_quote` form.
#[must_use]
pub fn split_base_quote(lbank_symbol: &str) -> Option<(String, String)> {
    let mut parts = lbank_symbol.split('_');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(base), Some(quote), None) if !base.is_empty() && !quote.is_empty() => {
            Some((base.to_uppercase(), quote.to_uppercase()))
        }
        _ => None,
    }
}

/// Converts an LBank CONTRACT symbol (`BTCUSDT`, no underscore) to a Nautilus [`InstrumentId`]
/// (`BTCUSDT.LBANK`). Contract symbols are uppercase and NOT underscore-delimited (unlike spot).
#[must_use]
pub fn instrument_id_from_contract_symbol(contract_symbol: &str) -> InstrumentId {
    InstrumentId::new(Symbol::from(contract_symbol.to_uppercase().as_str()), lbank_venue())
}

/// Converts a Nautilus [`InstrumentId`] back to the LBank contract symbol (`BTCUSDT`).
#[must_use]
pub fn contract_symbol_from_instrument_id(instrument_id: &InstrumentId) -> String {
    instrument_id.symbol.as_str().to_string()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("btc_usdt", "BTC_USDT.LBANK", "BTC", "USDT")]
    #[case("1inch_usdt", "1INCH_USDT.LBANK", "1INCH", "USDT")]
    #[case("eth_btc", "ETH_BTC.LBANK", "ETH", "BTC")]
    fn symbol_roundtrip(
        #[case] lbank: &str,
        #[case] expected_id: &str,
        #[case] base: &str,
        #[case] quote: &str,
    ) {
        let id = instrument_id_from_lbank_symbol(lbank);
        assert_eq!(id.to_string(), expected_id);
        assert_eq!(lbank_symbol_from_instrument_id(&id), lbank);
        assert_eq!(split_base_quote(lbank), Some((base.to_string(), quote.to_string())));
    }

    #[rstest]
    fn split_rejects_non_pair() {
        assert_eq!(split_base_quote("btcusdt"), None);
        assert_eq!(split_base_quote("a_b_c"), None);
    }
}
