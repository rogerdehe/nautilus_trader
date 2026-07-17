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

//! MEXC spot WebSocket client (control-plane; market-data frames are protobuf — see [`super`]).

use std::sync::Arc;

use nautilus_network::{
    Message,
    websocket::{MessageHandler, WebSocketClient, WebSocketConfig},
};

use super::messages::MexcWsRequest;
use crate::common::consts::MEXC_WS_SPOT_URL;

/// The MEXC spot WebSocket keepalive interval (seconds); CCXT `streaming.keepAlive` = 8000ms.
const MEXC_WS_HEARTBEAT_SECS: u64 = 8;

/// A lean MEXC spot WebSocket client.
///
/// Connects to the spot WS endpoint, sends `SUBSCRIPTION`/`UNSUBSCRIPTION` control frames and the
/// `{"method":"ping"}` keepalive. Incoming binary (protobuf) market-data frames are logged and
/// dropped (documented gap — see [`super`]); JSON control frames are logged.
#[derive(Debug)]
pub struct MexcWebSocketClient {
    url: String,
    inner: Option<WebSocketClient>,
}

impl MexcWebSocketClient {
    /// Creates a new client targeting `url` (defaults to the MEXC spot WS endpoint).
    #[must_use]
    pub fn new(url: Option<String>) -> Self {
        Self {
            url: url.unwrap_or_else(|| MEXC_WS_SPOT_URL.to_string()),
            inner: None,
        }
    }

    /// Returns `true` when the underlying connection is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.inner.as_ref().is_some_and(WebSocketClient::is_active)
    }

    /// Connects to the MEXC spot WebSocket with keepalive configured.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration is invalid or the connection fails.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let config = WebSocketConfig::builder()
            .url(self.url.clone())
            .heartbeat(MEXC_WS_HEARTBEAT_SECS)
            .heartbeat_msg(MexcWsRequest::ping().to_json())
            .build()
            .map_err(|e| anyhow::anyhow!("Invalid MEXC WebSocket config: {e}"))?;

        let handler: MessageHandler = Arc::new(|msg: Message| match msg {
            Message::Binary(_) => {
                // MEXC spot public channels are protobuf-encoded; not decoded yet (REST-poll fallback).
                log::trace!("MEXC WS: dropped binary (protobuf) frame — data via REST poll");
            }
            Message::Text(text) => log::trace!("MEXC WS control: {text:?}"),
            _ => {}
        });

        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .map_err(|e| anyhow::anyhow!("MEXC WebSocket connect failed: {e}"))?;

        self.inner = Some(client);
        Ok(())
    }

    /// Subscribes to the given (protobuf) channels.
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or the send fails.
    pub async fn subscribe(&self, channels: Vec<String>) -> anyhow::Result<()> {
        self.send(MexcWsRequest::subscription(channels)).await
    }

    /// Unsubscribes from the given channels.
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or the send fails.
    pub async fn unsubscribe(&self, channels: Vec<String>) -> anyhow::Result<()> {
        self.send(MexcWsRequest::unsubscription(channels)).await
    }

    async fn send(&self, request: MexcWsRequest) -> anyhow::Result<()> {
        let client = self
            .inner
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("MEXC WebSocket not connected"))?;
        client
            .send_text(request.to_json(), None)
            .await
            .map_err(|e| anyhow::anyhow!("MEXC WebSocket send failed: {e}"))
    }

    /// Disconnects the WebSocket, if connected.
    pub async fn close(&mut self) {
        if let Some(client) = self.inner.take() {
            client.disconnect().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_new_defaults_to_spot_url() {
        let client = MexcWebSocketClient::new(None);
        assert_eq!(client.url, MEXC_WS_SPOT_URL);
        assert!(!client.is_active());
    }

    #[rstest]
    fn test_new_custom_url() {
        let client = MexcWebSocketClient::new(Some("wss://custom/ws".to_string()));
        assert_eq!(client.url, "wss://custom/ws");
    }
}
