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

//! Error types for the LBank HTTP client.
//!
//! LBank returns **HTTP 200 with `error_code != 0`** for logical failures (e.g. `10007` invalid
//! signature, `10004` throttled, `10031` bad echostr). The client therefore inspects the response
//! envelope's `error_code` in addition to the transport status.

use nautilus_network::http::HttpClientError;

/// Result alias for the LBank HTTP layer.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the LBank HTTP client.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Missing or invalid credentials for a signed request.
    #[error("authentication error: {0}")]
    Auth(String),
    /// A non-zero `error_code` in an otherwise HTTP-200 response envelope.
    #[error("LBank API error {code}: {msg}")]
    Api {
        /// LBank `error_code`.
        code: i64,
        /// Human-readable message (from `msg`, or a code lookup).
        msg: String,
    },
    /// A non-success HTTP status was returned by the venue.
    #[error("HTTP status {status}: {body}")]
    Status {
        /// HTTP status code.
        status: u16,
        /// Response body.
        body: String,
    },
    /// The envelope was successful but carried no `data`.
    #[error("missing data in LBank response")]
    MissingData,
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

    /// Builds an [`Error::Api`] from a code, resolving a friendly message when `msg` is absent.
    #[must_use]
    pub fn api(code: i64, msg: Option<String>) -> Self {
        let msg = msg
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| error_code_message(code).to_string());
        Self::Api { code, msg }
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

    /// Returns `true` when the error is safe to retry (transport, 5xx/429, or throttle code).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Status { status, .. } => *status == 429 || *status >= 500,
            Self::Api { code, .. } => *code == 10004, // request too frequent
            _ => false,
        }
    }
}

/// Maps a known LBank spot `error_code` to a human-readable message (from CCXT `handle_errors`).
#[must_use]
pub fn error_code_message(code: i64) -> &'static str {
    match code {
        10000 => "internal error",
        10001 => "required parameters cannot be empty",
        10002 => "validation failed",
        10003 => "invalid parameter",
        10004 => "request too frequent",
        10005 => "secret key does not exist",
        10006 => "user does not exist",
        10007 => "invalid signature",
        10011 => "illegal IP",
        10016 => "insufficient balance",
        10025 => "order does not exist",
        10031 => "invalid echostr length",
        10036 => "order already exists",
        _ => "unknown LBank error",
    }
}
