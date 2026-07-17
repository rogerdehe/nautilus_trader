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

//! MEXC spot WebSocket channel names and JSON control messages (first-hand from CCXT `pro/mexc`).

use serde::{Deserialize, Serialize};

/// Order book depth update frequency (`params.frequency` in CCXT `watch_order_book`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DepthFrequency {
    /// 10 millisecond updates.
    Ms10,
    /// 100 millisecond updates (default).
    Ms100,
}

impl DepthFrequency {
    /// Returns the MEXC wire value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ms10 => "10ms",
            Self::Ms100 => "100ms",
        }
    }
}

/// Builds the protobuf aggregate-depth channel name for a symbol, e.g.
/// `spot@public.aggre.depth.v3.api.pb@100ms@BTCUSDT`.
#[must_use]
pub fn depth_channel(symbol: &str, freq: DepthFrequency) -> String {
    format!("spot@public.aggre.depth.v3.api.pb@{}@{symbol}", freq.as_str())
}

/// Builds the protobuf aggregate-deals (trades) channel name for a symbol, e.g.
/// `spot@public.aggre.deals.v3.api.pb@100ms@BTCUSDT`.
#[must_use]
pub fn deals_channel(symbol: &str) -> String {
    format!("spot@public.aggre.deals.v3.api.pb@100ms@{symbol}")
}

/// Builds the protobuf aggregate book-ticker channel name for a symbol, e.g.
/// `spot@public.aggre.bookTicker.v3.api.pb@100ms@BTCUSDT`.
#[must_use]
pub fn book_ticker_channel(symbol: &str) -> String {
    format!("spot@public.aggre.bookTicker.v3.api.pb@100ms@{symbol}")
}

/// Builds the protobuf kline channel name for a symbol and MEXC interval, e.g.
/// `spot@public.kline.v3.api.pb@BTCUSDT@Min1`.
#[must_use]
pub fn kline_channel(symbol: &str, interval: &str) -> String {
    format!("spot@public.kline.v3.api.pb@{symbol}@{interval}")
}

/// A MEXC spot WebSocket subscription request (JSON control frame).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MexcWsRequest {
    /// `"SUBSCRIPTION"`, `"UNSUBSCRIPTION"`, or `"ping"`.
    pub method: String,
    /// The channels to (un)subscribe to (omitted for `ping`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<String>,
}

impl MexcWsRequest {
    /// Builds a `SUBSCRIPTION` request for the given channels.
    #[must_use]
    pub fn subscription(params: Vec<String>) -> Self {
        Self {
            method: "SUBSCRIPTION".to_string(),
            params,
        }
    }

    /// Builds an `UNSUBSCRIPTION` request for the given channels.
    #[must_use]
    pub fn unsubscription(params: Vec<String>) -> Self {
        Self {
            method: "UNSUBSCRIPTION".to_string(),
            params,
        }
    }

    /// Builds a `ping` keepalive request.
    #[must_use]
    pub fn ping() -> Self {
        Self {
            method: "ping".to_string(),
            params: Vec::new(),
        }
    }

    /// Serializes this request to a JSON string.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("MexcWsRequest serialization cannot fail")
    }
}

/// A MEXC spot WebSocket JSON control response, e.g. a subscribe ack
/// `{"id":0,"code":0,"msg":"spot@public.aggre.depth.v3.api.pb@100ms@BTCUSDT"}`.
#[derive(Clone, Debug, Deserialize)]
pub struct MexcWsControl {
    /// The response id.
    #[serde(default)]
    pub id: Option<i64>,
    /// The response code (`0` = success).
    #[serde(default)]
    pub code: Option<i64>,
    /// The response message (echoes the channel on ack, or an error).
    #[serde(default)]
    pub msg: Option<String>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_channel_names() {
        assert_eq!(
            depth_channel("BTCUSDT", DepthFrequency::Ms100),
            "spot@public.aggre.depth.v3.api.pb@100ms@BTCUSDT"
        );
        assert_eq!(
            deals_channel("BTCUSDT"),
            "spot@public.aggre.deals.v3.api.pb@100ms@BTCUSDT"
        );
        assert_eq!(
            kline_channel("BTCUSDT", "Min1"),
            "spot@public.kline.v3.api.pb@BTCUSDT@Min1"
        );
    }

    #[rstest]
    fn test_subscription_json() {
        let req = MexcWsRequest::subscription(vec![deals_channel("BTCUSDT")]);
        let json = req.to_json();
        assert_eq!(
            json,
            r#"{"method":"SUBSCRIPTION","params":["spot@public.aggre.deals.v3.api.pb@100ms@BTCUSDT"]}"#
        );
    }

    #[rstest]
    fn test_ping_json() {
        assert_eq!(MexcWsRequest::ping().to_json(), r#"{"method":"ping"}"#);
    }

    #[rstest]
    fn test_control_ack_parse() {
        let ack: MexcWsControl = serde_json::from_str(
            r#"{"id":0,"code":0,"msg":"spot@public.aggre.depth.v3.api.pb@100ms@BTCUSDT"}"#,
        )
        .unwrap();
        assert_eq!(ack.code, Some(0));
    }
}
