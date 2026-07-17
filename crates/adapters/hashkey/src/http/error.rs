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

//! Error types for the HashKey HTTP client.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// HashKey error envelope (`{"code": "...", "msg": "..."}`) — see CCXT `handle_errors`.
///
/// HashKey returns a numeric `code` (sometimes as a string, sometimes as an int) and a `msg`
/// string on failures; successful data responses have no envelope.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HashKeyErrorResponse {
    /// The HashKey error code (e.g. `"0002"`, `-1021`). Kept as raw JSON for cross-type tolerance.
    pub code: serde_json::Value,
    /// The human-readable error message.
    #[serde(default)]
    pub msg: String,
}

/// Errors returned by the HashKey HTTP client.
#[derive(Debug, Error)]
pub enum HashKeyHttpError {
    /// Credentials were required for a private endpoint but none were configured.
    #[error("Missing credentials for authenticated HashKey request")]
    MissingCredentials,
    /// A validation / construction error before the request was sent.
    #[error("Validation error: {0}")]
    ValidationError(String),
    /// The underlying network transport failed.
    #[error("Network error: {0}")]
    NetworkError(String),
    /// JSON (de)serialization failed.
    #[error("JSON error: {0}")]
    JsonError(String),
    /// HashKey returned a business error envelope.
    #[error("HashKey error [{code}]: {message}")]
    HashKeyError {
        /// The raw error code string.
        code: String,
        /// The error message.
        message: String,
    },
    /// An unexpected non-success HTTP status.
    #[error("Unexpected HTTP status {status}: {body}")]
    UnexpectedStatus {
        /// The HTTP status code.
        status: u16,
        /// The response body.
        body: String,
    },
}
