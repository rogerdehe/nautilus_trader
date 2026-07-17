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

//! BingX live data client (spot market data over REST + WebSocket).

use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use nautilus_common::{
    clients::DataClient,
    live::runner::get_data_event_sender,
    messages::{
        DataEvent,
        data::{SubscribeBookDeltas, SubscribeTrades, UnsubscribeBookDeltas, UnsubscribeTrades},
    },
    providers::InstrumentProvider,
};
use nautilus_model::{
    identifiers::{ClientId, Venue},
    instruments::Instrument,
};

use crate::{
    common::consts::bingx_venue,
    config::BingXDataClientConfig,
    http::client::{BingXHttpClient, BingXInstrumentProvider},
    websocket::client::BingXWebSocketClient,
};

/// Default order book depth (levels) to subscribe for (`watchOrderBook.depth` in CCXT = 100).
const DEFAULT_BOOK_LEVELS: u32 = 100;

/// Live data client for BingX spot market data.
pub struct BingXDataClient {
    client_id: ClientId,
    config: BingXDataClientConfig,
    provider: BingXInstrumentProvider,
    ws: BingXWebSocketClient,
    is_connected: AtomicBool,
}

impl std::fmt::Debug for BingXDataClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BingXDataClient")
            .field("client_id", &self.client_id)
            .field("product_type", &self.config.product_type)
            .finish_non_exhaustive()
    }
}

impl BingXDataClient {
    /// Creates a new [`BingXDataClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP/WebSocket clients cannot be constructed.
    pub fn new(client_id: ClientId, config: BingXDataClientConfig) -> anyhow::Result<Self> {
        let http_client = if config.has_api_credentials() {
            BingXHttpClient::with_credentials(
                config.api_key.clone().unwrap_or_default(),
                config.api_secret.clone().unwrap_or_default(),
                Some(config.http_base_url()),
                Some(config.http_timeout_secs),
            )
        } else {
            BingXHttpClient::new(Some(config.http_base_url()), Some(config.http_timeout_secs))
        };

        let provider = BingXInstrumentProvider::new(http_client);
        let data_sender = get_data_event_sender();
        let ws = BingXWebSocketClient::new(config.ws_url(), config.product_type, data_sender);

        Ok(Self {
            client_id,
            config,
            provider,
            ws,
            is_connected: AtomicBool::new(false),
        })
    }
}

#[async_trait(?Send)]
impl DataClient for BingXDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(bingx_venue())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.ws.close();
        self.is_connected.store(false, Ordering::Release);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.ws.close();
        self.is_connected.store(false, Ordering::Release);
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Acquire)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        // Load spot instruments and register their precisions with the WS read loop so it can
        // quantize venue price/size strings.
        self.provider.load_all(None).await?;
        let data_sender = get_data_event_sender();
        for instrument in self.provider.store().get_all().values() {
            self.ws.add_instrument(
                instrument.id(),
                instrument.price_precision(),
                instrument.size_precision(),
            );
            // Publish the instrument to the platform.
            if let Err(e) = data_sender.send(DataEvent::Instrument(instrument.clone())) {
                log::error!("Failed to emit BingX instrument: {e}");
            }
        }

        self.ws.connect().await?;
        self.is_connected.store(true, Ordering::Release);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.ws.close();
        self.is_connected.store(false, Ordering::Release);
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        self.ws.subscribe_book(cmd.instrument_id, DEFAULT_BOOK_LEVELS)
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        self.ws.subscribe_trades(cmd.instrument_id)
    }

    fn unsubscribe_book_deltas(&mut self, _cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        // BingX unsubscribe uses reqType=unsub; not yet wired (see TODOs in the adapter notes).
        Ok(())
    }

    fn unsubscribe_trades(&mut self, _cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        Ok(())
    }
}
