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

//! Error types for the Gate HTTP client.

use nautilus_network::http::HttpClientError;

/// Result alias for the Gate HTTP layer.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the Gate HTTP client.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Missing or invalid credentials for a signed request.
    #[error("authentication error: {0}")]
    Auth(String),
    /// A non-success HTTP status was returned by the venue.
    #[error("HTTP status {status}: {body}")]
    Status {
        /// HTTP status code.
        status: u16,
        /// Response body (label + message from Gate's error envelope when present).
        body: String,
    },
    /// Transport-level failure (connection, timeout, etc.).
    #[error("transport error: {0}")]
    Transport(String),
    /// JSON (de)serialization failure.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

impl Error {
    /// Builds an [`Error::Auth`].
    #[must_use]
    pub fn auth(msg: impl Into<String>) -> Self {
        Self::Auth(msg.into())
    }

    /// Builds an [`Error::Transport`].
    #[must_use]
    pub fn transport(msg: impl Into<String>) -> Self {
        Self::Transport(msg.into())
    }

    /// Builds an [`Error::Status`] from an HTTP status and raw body bytes.
    #[must_use]
    pub fn from_http_status(status: u16, body: &[u8]) -> Self {
        Self::Status {
            status,
            body: String::from_utf8_lossy(body).into_owned(),
        }
    }

    /// Maps a [`HttpClientError`] into a transport error.
    #[must_use]
    pub fn from_http_client(err: HttpClientError) -> Self {
        Self::Transport(err.to_string())
    }

    /// Returns `true` when the error is safe to retry (transport + 5xx / 429).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Status { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }
}
