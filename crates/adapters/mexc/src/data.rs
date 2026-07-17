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

//! Live market-data client for the MEXC adapter.
//!
//! MEXC spot WebSocket market data is protobuf-encoded (documented gap — see [`crate::websocket`]),
//! so this client sources instruments and trades via **REST** (`request_*`). Streaming
//! subscriptions (`subscribe_*`) are not yet supported and log a clear gap warning.

use std::sync::atomic::{AtomicBool, Ordering};

use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent,
        data::{
            DataResponse, InstrumentsResponse, RequestInstrument, RequestInstruments, RequestTrades,
            SubscribeBookDeltas, SubscribeQuotes, SubscribeTrades, TradesResponse,
        },
    },
};
use nautilus_core::{
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::identifiers::{ClientId, Venue};

use crate::{
    common::consts::MEXC_VENUE,
    config::MexcDataClientConfig,
    http::client::MexcHttpClient,
};

/// A live market-data client for MEXC spot (REST-backed; see module docs).
#[derive(Debug)]
pub struct MexcDataClient {
    client_id: ClientId,
    /// Retained for reconnection / future REST-poll scheduling.
    #[allow(dead_code)]
    config: MexcDataClientConfig,
    http_client: MexcHttpClient,
    is_connected: AtomicBool,
    clock: &'static AtomicTime,
}

impl MexcDataClient {
    /// Creates a new [`MexcDataClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(client_id: ClientId, config: MexcDataClientConfig) -> anyhow::Result<Self> {
        let http_client = MexcHttpClient::new(
            Some(config.http_base_url()),
            Some(config.http_timeout_secs),
        )
        .map_err(|e| anyhow::anyhow!("Failed to create MEXC HTTP client: {e}"))?;

        Ok(Self {
            client_id,
            config,
            http_client,
            is_connected: AtomicBool::new(false),
            clock: get_atomic_clock_realtime(),
        })
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for MexcDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*MEXC_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Acquire)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        // Warm the instrument cache so precision lookups are populated for trade parsing.
        let ts_init = self.clock.get_time_ns();
        if let Err(e) = self.http_client.request_instruments(ts_init).await {
            log::warn!("MEXC: failed to preload instruments on connect: {e}");
        }
        self.is_connected.store(true, Ordering::Release);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.is_connected.store(false, Ordering::Release);
        Ok(())
    }

    fn subscribe_trades(&mut self, _cmd: SubscribeTrades) -> anyhow::Result<()> {
        log::warn!(
            "MEXC: subscribe_trades not supported — spot WS market data is protobuf-encoded (gap); use request_trades (REST poll)"
        );
        Ok(())
    }

    fn subscribe_quotes(&mut self, _cmd: SubscribeQuotes) -> anyhow::Result<()> {
        log::warn!(
            "MEXC: subscribe_quotes not supported — spot WS market data is protobuf-encoded (gap)"
        );
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, _cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        log::warn!(
            "MEXC: subscribe_book_deltas not supported — spot WS market data is protobuf-encoded (gap)"
        );
        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = get_data_event_sender();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.request_instruments(clock.get_time_ns()).await {
                Ok(instruments) => {
                    let response = DataResponse::Instruments(InstrumentsResponse::new(
                        request_id,
                        client_id,
                        *MEXC_VENUE,
                        instruments,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));
                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("MEXC: failed to send instruments response: {e}");
                    }
                }
                Err(e) => log::error!("MEXC: instruments request failed: {e}"),
            }
        });
        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = get_data_event_sender();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let instrument_id = request.instrument_id;
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.request_instruments(clock.get_time_ns()).await {
                Ok(instruments) => {
                    if let Some(inst) = instruments
                        .into_iter()
                        .find(|i| nautilus_model::instruments::Instrument::id(i) == instrument_id)
                    {
                        let response = DataResponse::Instrument(Box::new(
                            nautilus_common::messages::data::InstrumentResponse::new(
                                request_id,
                                client_id,
                                instrument_id,
                                inst,
                                start_nanos,
                                end_nanos,
                                clock.get_time_ns(),
                                params,
                            ),
                        ));
                        if let Err(e) = sender.send(DataEvent::Response(response)) {
                            log::error!("MEXC: failed to send instrument response: {e}");
                        }
                    } else {
                        log::warn!("MEXC: instrument {instrument_id} not found");
                    }
                }
                Err(e) => log::error!("MEXC: instrument request failed: {e}"),
            }
        });
        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = get_data_event_sender();
        let instrument_id = request.instrument_id;
        let limit = request.limit.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.request_trades(instrument_id, limit, clock.get_time_ns()).await {
                Ok(trades) => {
                    let response = DataResponse::Trades(TradesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        trades,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));
                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("MEXC: failed to send trades response: {e}");
                    }
                }
                Err(e) => log::error!("MEXC: trades request failed: {e}"),
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_data_client_new() {
        let client =
            MexcDataClient::new(ClientId::from("MEXC"), MexcDataClientConfig::default()).unwrap();
        assert_eq!(client.client_id(), ClientId::from("MEXC"));
        assert_eq!(client.venue(), Some(*MEXC_VENUE));
        assert!(client.is_disconnected());
    }
}
