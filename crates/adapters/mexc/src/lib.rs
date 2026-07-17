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

//! [NautilusTrader](http://nautilustrader.io) integration adapter for the MEXC crypto exchange.
//!
//! Ported first-hand from the CCXT `mexc` implementation
//! (see `docs/CCXT_TO_NAUTILUS_ADAPTER_PLAYBOOK.md`).
//!
//! # Scope
//!
//! This adapter targets the **spot** market (`BTCUSDT` style symbols) using the MEXC v3 REST API.
//!
//! # WebSocket gap (documented)
//!
//! MEXC's spot WebSocket (`wss://wbs-api.mexc.com/ws`) delivers all public market-data channels
//! (`spot@public.aggre.depth.v3.api.pb`, `spot@public.aggre.deals.v3.api.pb`, ...) as **Protocol
//! Buffers** binary frames, not JSON. Porting the protobuf schema is out of scope for the initial
//! spot data path, so the market-data path falls back to REST polling (see [`data`]). The
//! WebSocket client here connects and manages subscriptions/keepalive but does not decode the
//! protobuf push frames yet.

#![warn(rustc::all)]
#![deny(unsafe_code)]

pub mod common;
pub mod config;
pub mod data;
pub mod execution;
pub mod factories;
pub mod http;
pub mod websocket;
