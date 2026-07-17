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

//! Bitget [`DataClient`] implementation (spot + USDT-M market data).

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent,
        data::{SubscribeBookDeltas, SubscribeInstruments, SubscribeTrades},
    },
    providers::InstrumentProvider,
};
use nautilus_model::{
    identifiers::{ClientId, Venue},
    instruments::Instrument,
};
use nautilus_network::websocket::WebSocketClient;

use crate::{
    common::consts::BITGET_VENUE,
    config::BitgetDataClientConfig,
    http::client::BitgetHttpClient,
    websocket::{
        client::BitgetWebSocketClient,
        messages::{BITGET_BOOK_CHANNEL, BITGET_TRADE_CHANNEL},
    },
};

/// Data client for Bitget spot + USDT-M market data over v2 REST + WebSocket.
pub struct BitgetDataClient {
    client_id: ClientId,
    http_client: BitgetHttpClient,
    ws: BitgetWebSocketClient,
    ws_handle: Option<Arc<WebSocketClient>>,
    is_connected: AtomicBool,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
}

impl std::fmt::Debug for BitgetDataClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BitgetDataClient")
            .field("client_id", &self.client_id)
            .field("is_connected", &self.is_connected.load(Ordering::Relaxed))
            .finish()
    }
}

impl BitgetDataClient {
    /// Creates a new [`BitgetDataClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP or WebSocket client cannot be constructed.
    pub fn new(client_id: ClientId, config: BitgetDataClientConfig) -> anyhow::Result<Self> {
        let http_client = if config.has_api_credentials() {
            BitgetHttpClient::with_credentials(
                config.api_key.clone(),
                config.api_secret.clone(),
                config.api_passphrase.clone(),
                Some(config.http_base_url()),
                Some(config.http_timeout_secs),
            )?
        } else {
            BitgetHttpClient::new(Some(config.http_base_url()), Some(config.http_timeout_secs))?
        };

        let ws = BitgetWebSocketClient::new(config.base_url_ws_public.clone());

        Ok(Self {
            client_id,
            http_client,
            ws,
            ws_handle: None,
            is_connected: AtomicBool::new(false),
            data_sender: get_data_event_sender(),
        })
    }

    fn spawn_subscription(&self, instrument_id: nautilus_model::identifiers::InstrumentId, channel: &str) {
        let Some(handle) = self.ws_handle.clone() else {
            log::warn!("Bitget data client not connected; dropping subscribe for {instrument_id}");
            return;
        };
        let channel = channel.to_string();
        get_runtime().spawn(async move {
            match BitgetWebSocketClient::subscription_text(instrument_id, &channel) {
                Ok(text) => {
                    if let Err(e) = handle.send_text(text, None).await {
                        log::error!("Bitget subscribe failed for {instrument_id}: {e}");
                    }
                }
                Err(e) => log::error!("Bitget subscribe encode failed for {instrument_id}: {e}"),
            }
        });
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for BitgetDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*BITGET_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!("Started Bitget data client {}", self.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Bitget data client {}", self.client_id);
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.stop()
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

        // Load instruments and publish to the engine; register precisions for the WS read loop.
        self.http_client.load_all(None).await?;
        let precisions = self.ws.precisions();
        let instruments: Vec<_> = self
            .http_client
            .store()
            .get_all()
            .values()
            .cloned()
            .collect();
        for inst in instruments {
            precisions.insert(
                inst.id(),
                (inst.price_precision(), inst.size_precision()),
            );
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(inst)) {
                log::error!("Failed to publish Bitget instrument: {e}");
            }
        }

        // Connect the public WebSocket and forward parsed data into the data engine.
        self.ws.connect().await?;
        self.ws_handle = self.ws.inner_handle();
        if let Some(mut rx) = self.ws.take_stream() {
            let sender = self.data_sender.clone();
            get_runtime().spawn(async move {
                while let Some(data) = rx.recv().await {
                    if let Err(e) = sender.send(DataEvent::Data(data)) {
                        log::error!("Bitget data forward failed: {e}");
                        break;
                    }
                }
            });
        }

        self.is_connected.store(true, Ordering::Relaxed);
        log::info!("Connected Bitget data client {}", self.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.ws.close().await;
        self.ws_handle = None;
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn subscribe_instruments(&mut self, _cmd: SubscribeInstruments) -> anyhow::Result<()> {
        // Instruments are published on connect; nothing further to do.
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        self.spawn_subscription(cmd.instrument_id, BITGET_BOOK_CHANNEL);
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        self.spawn_subscription(cmd.instrument_id, BITGET_TRADE_CHANNEL);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // NOTE: `BitgetDataClient::new` calls `get_data_event_sender()` (as OKX does), which requires
    // an initialized live runner. Full construction is therefore exercised in the live node /
    // factory integration path, not in a bare unit test. Here we assert the config wiring only.
    #[rstest]
    fn test_data_config_credentials_gate() {
        let config = BitgetDataClientConfig::default();
        // Without env vars set, a default config has no credentials.
        let (k, s, p) = crate::common::credential::credential_env_vars();
        if std::env::var(k).is_err() && std::env::var(s).is_err() && std::env::var(p).is_err() {
            assert!(!config.has_api_credentials());
        }
        assert_eq!(config.http_base_url(), crate::common::consts::BITGET_HTTP_URL);
    }
}
