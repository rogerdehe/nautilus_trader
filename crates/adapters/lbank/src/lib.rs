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

//! [LBank](https://www.lbank.com) integration adapter for the NautilusTrader platform.
//!
//! Spot market data + execution over LBank's v2 REST + WebSocket API. Signing, endpoints and message
//! shapes are implemented first-hand against the CCXT reference (`ccxt/python/ccxt/lbank.py` +
//! `pro/lbank.py`): HmacSHA256-over-uppercased-MD5 request signing, `btc_usdt` symbol format,
//! `wss://www.lbkex.net/ws/V2/` full-snapshot depth + trade streams.

pub mod common;
pub mod config;
pub mod contract_data;
pub mod data;
pub mod execution;
pub mod factories;
pub mod http;
pub mod provider;
pub mod websocket;
