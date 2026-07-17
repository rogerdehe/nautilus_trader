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

//! KuCoin symbol <-> Nautilus identifier conversion.
//!
//! KuCoin spot pairs are uppercase `BASE-QUOTE` with a dash (`BTC-USDT`). The mapping to Nautilus
//! keeps the dash so it is REVERSIBLE and unambiguous (collapsing to `BTCUSDT` would be lossy).
//! So `BTC-USDT` <-> `BTC-USDT.KUCOIN`.

use nautilus_model::identifiers::{InstrumentId, Symbol};

use crate::common::consts::kucoin_venue;

/// Converts a KuCoin pair (`BTC-USDT`) to a Nautilus [`InstrumentId`] (`BTC-USDT.KUCOIN`).
#[must_use]
pub fn instrument_id_from_kucoin_symbol(kucoin_symbol: &str) -> InstrumentId {
    InstrumentId::new(Symbol::from(kucoin_symbol), kucoin_venue())
}

/// Converts a Nautilus [`InstrumentId`] back to the KuCoin pair string (`BTC-USDT`).
#[must_use]
pub fn kucoin_symbol_from_instrument_id(instrument_id: &InstrumentId) -> String {
    instrument_id.symbol.as_str().to_string()
}

/// Splits a KuCoin pair into `(base, quote)` uppercased currency codes, or `None` if the pair is
/// not a single `BASE-QUOTE` form.
#[must_use]
pub fn split_base_quote(kucoin_symbol: &str) -> Option<(String, String)> {
    let mut parts = kucoin_symbol.split('-');
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
    #[case("BTC-USDT", "BTC-USDT.KUCOIN", "BTC", "USDT")]
    #[case("KCS-BTC", "KCS-BTC.KUCOIN", "KCS", "BTC")]
    #[case("1INCH-USDT", "1INCH-USDT.KUCOIN", "1INCH", "USDT")]
    fn symbol_roundtrip(
        #[case] kucoin: &str,
        #[case] expected_id: &str,
        #[case] base: &str,
        #[case] quote: &str,
    ) {
        let id = instrument_id_from_kucoin_symbol(kucoin);
        assert_eq!(id.to_string(), expected_id);
        assert_eq!(kucoin_symbol_from_instrument_id(&id), kucoin);
        assert_eq!(
            split_base_quote(kucoin),
            Some((base.to_string(), quote.to_string()))
        );
    }

    #[rstest]
    fn split_rejects_non_pair() {
        assert_eq!(split_base_quote("BTCUSDT"), None);
        assert_eq!(split_base_quote("A-B-C"), None);
    }
}
