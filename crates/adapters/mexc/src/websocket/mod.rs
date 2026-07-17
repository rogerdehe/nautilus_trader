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

//! MEXC spot WebSocket client and message builders.
//!
//! # Protobuf gap (IMPORTANT)
//!
//! MEXC's current spot WebSocket (`wss://wbs-api.mexc.com/ws`) delivers **all** public market-data
//! push frames as Protocol Buffers binary (channels suffixed `.pb@`, e.g.
//! `spot@public.aggre.depth.v3.api.pb@100ms@BTCUSDT`). CCXT decodes them with a bundled protobuf
//! schema (`decode_proto_msg` / `handle_protobuf_message`). Porting that schema to Rust is out of
//! scope for the initial spot data path.
//!
//! Consequences:
//! - The *control plane* is JSON: `SUBSCRIPTION` / `UNSUBSCRIPTION` requests, `{"method":"ping"}`
//!   keepalive, and subscribe acknowledgements (`{"id":0,"code":0,"msg":...}`) are all JSON and are
//!   handled here.
//! - The *data plane* (depth/deals/bookTicker/kline pushes) is protobuf binary and is **not decoded**
//!   here — binary frames are logged and dropped. Market data is instead sourced via REST polling in
//!   [`crate::data`].
//!
//! This module therefore provides a real connection + subscription/keepalive scaffold plus the
//! channel/message builders, ready for a future protobuf decode layer.

pub mod client;
pub mod messages;
