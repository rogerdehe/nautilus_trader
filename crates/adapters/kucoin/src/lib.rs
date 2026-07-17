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

//! [KuCoin](https://www.kucoin.com) integration adapter for the NautilusTrader platform.
//!
//! Spot market data + execution over KuCoin's REST + WebSocket API. Signing, endpoints and message
//! shapes are implemented first-hand against the CCXT reference (`ccxt/python/ccxt/kucoin.py` +
//! `pro/kucoin.py`): `KC-API-KEY-VERSION: 2` HMAC-SHA256 base64 request signing, `BTC-USDT` symbol
//! format, and the bullet-token WebSocket handshake (`/spotMarket/level2Depth50` snapshots +
//! `/market/match` trades + JSON ping/pong).

#![warn(rustc::all)]
#![deny(nonstandard_style)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod common;
pub mod config;
pub mod data;
pub mod execution;
pub mod factories;
pub mod http;
pub mod websocket;

// Re-exports
pub use crate::{
    config::{KuCoinDataClientConfig, KuCoinExecClientConfig},
    data::KuCoinDataClient,
    execution::KuCoinExecutionClient,
    factories::{KuCoinDataClientFactory, KuCoinExecClientFactory},
    http::{client::KuCoinHttpClient, error::KuCoinHttpError},
    websocket::client::KuCoinWebSocketClient,
};
