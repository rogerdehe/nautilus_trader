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

//! Live market data client for HashKey Global (spot data path).

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use async_trait::async_trait;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent,
        data::{SubscribeBookDeltas, SubscribeTrades, UnsubscribeBookDeltas, UnsubscribeTrades},
    },
};
use nautilus_model::{
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::WebSocketClient;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    common::{consts::hashkey_venue, parse::hashkey_symbol_from_instrument_id},
    config::HashKeyDataClientConfig,
    http::HashKeyHttpClient,
    websocket::{HashKeyWebSocketClient, messages::HashKeyWsMessage},
};

/// A live market data client for the HashKey Global exchange (spot).
pub struct HashKeyDataClient {
    client_id: ClientId,
    config: HashKeyDataClientConfig,
    http_client: HashKeyHttpClient,
    ws: HashKeyWebSocketClient,
    ws_sender: Option<Arc<WebSocketClient>>,
    is_connected: AtomicBool,
    data_sender: UnboundedSender<DataEvent>,
    instruments: AHashMap<InstrumentId, InstrumentAny>,
}

impl std::fmt::Debug for HashKeyDataClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(HashKeyDataClient))
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

impl HashKeyDataClient {
    /// Creates a new [`HashKeyDataClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be constructed.
    pub fn new(client_id: ClientId, config: HashKeyDataClientConfig) -> anyhow::Result<Self> {
        let data_sender = get_data_event_sender();

        let http_client = if config.has_api_credentials() {
            HashKeyHttpClient::with_credentials(
                config.api_key.clone(),
                config.api_secret.clone(),
                Some(config.http_base_url()),
                Some(config.http_timeout_secs),
            )?
        } else {
            HashKeyHttpClient::new(Some(config.http_base_url()), Some(config.http_timeout_secs))?
        };

        let ws = HashKeyWebSocketClient::new(config.ws_public_url(), config.transport_backend, None);

        Ok(Self {
            client_id,
            config,
            http_client,
            ws,
            ws_sender: None,
            is_connected: AtomicBool::new(false),
            data_sender,
            instruments: AHashMap::new(),
        })
    }

    fn subscribe_topic(&self, instrument_id: InstrumentId, topic: &'static str) {
        let Some(client) = self.ws_sender.clone() else {
            log::warn!("Cannot subscribe {topic}: HashKey websocket not connected");
            return;
        };
        let symbol = hashkey_symbol_from_instrument_id(&instrument_id);
        let frame = HashKeyWebSocketClient::subscribe_frame(&symbol, topic);
        get_runtime().spawn(async move {
            if let Err(e) = client.send_text(frame, None).await {
                log::error!("Failed to subscribe {topic} for {symbol}: {e}");
            }
        });
    }
}

#[async_trait(?Send)]
impl DataClient for HashKeyDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(hashkey_venue())
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
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        // Load instruments so the WS read loop and downstream consumers have precision metadata.
        let instruments = self.http_client.request_instruments().await?;
        self.instruments = instruments
            .iter()
            .map(|i| (i.id(), i.clone()))
            .collect();
        self.ws.cache_instruments(&instruments);

        for instrument in instruments {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::warn!("Failed to emit instrument: {e}");
            }
        }

        self.ws.connect().await?;
        self.ws.wait_until_active(10.0).await?;
        self.ws_sender = self.ws.client();

        let stream = self.ws.stream();
        let sender = self.data_sender.clone();
        get_runtime().spawn(async move {
            pin_mut!(stream);
            while let Some(msg) = stream.next().await {
                match msg {
                    HashKeyWsMessage::Data(items) => {
                        for data in items {
                            if let Err(e) = sender.send(DataEvent::Data(data)) {
                                log::error!("Failed to emit data event: {e}");
                                return;
                            }
                        }
                    }
                }
            }
        });

        self.is_connected.store(true, Ordering::Relaxed);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Ok(());
        }
        self.ws.disconnect().await;
        self.ws_sender = None;
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        self.subscribe_topic(cmd.instrument_id, "trade");
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        self.subscribe_topic(cmd.instrument_id, "depth");
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        log::debug!(
            "HashKey unsubscribe_trades for {} not yet implemented",
            cmd.instrument_id
        );
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        log::debug!(
            "HashKey unsubscribe_book_deltas for {} not yet implemented",
            cmd.instrument_id
        );
        Ok(())
    }
}

impl HashKeyDataClient {
    /// Returns the configured venue.
    #[must_use]
    pub fn config(&self) -> &HashKeyDataClientConfig {
        &self.config
    }
}
