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

//! Error types for the Bitget HTTP client.

use thiserror::Error;

/// Errors that can occur when using the Bitget HTTP client.
#[derive(Debug, Error)]
pub enum BitgetHttpError {
    /// Credentials are required for a private endpoint but were not configured.
    #[error("Missing credentials for authenticated request")]
    MissingCredentials,
    /// Input/validation error building the request.
    #[error("Validation error: {0}")]
    ValidationError(String),
    /// The underlying network transport failed.
    #[error("Network error: {0}")]
    NetworkError(String),
    /// JSON (de)serialization failed.
    #[error("JSON error: {0}")]
    JsonError(String),
    /// The Bitget API returned a business error (`code != "00000"`).
    #[error("Bitget error [{code}]: {message}")]
    BitgetError { code: String, message: String },
    /// An unexpected HTTP status was returned.
    #[error("Unexpected HTTP status {status}: {body}")]
    UnexpectedStatus { status: u16, body: String },
}

impl From<nautilus_network::http::HttpClientError> for BitgetHttpError {
    fn from(err: nautilus_network::http::HttpClientError) -> Self {
        Self::NetworkError(err.to_string())
    }
}

impl From<serde_json::Error> for BitgetHttpError {
    fn from(err: serde_json::Error) -> Self {
        Self::JsonError(err.to_string())
    }
}
