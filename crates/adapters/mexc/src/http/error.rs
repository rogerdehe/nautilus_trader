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

//! MEXC HTTP error types.

use serde::Deserialize;

/// MEXC spot error envelope, e.g. `{"code":-1121,"msg":"Invalid symbol."}`.
#[derive(Clone, Debug, Deserialize)]
pub struct MexcErrorResponse {
    /// The MEXC error code (negative for spot business errors).
    pub code: i64,
    /// The error message (`msg` on spot, `message` on some v1 responses).
    #[serde(alias = "message")]
    pub msg: Option<String>,
}

/// Errors returned by the MEXC HTTP client.
#[derive(Debug, thiserror::Error)]
pub enum MexcHttpError {
    /// Credentials are required for a private endpoint but were not provided.
    #[error("Missing credentials for authenticated MEXC request")]
    MissingCredentials,
    /// The MEXC API returned a business error.
    #[error("MEXC error {code}: {message}")]
    MexcError {
        /// The MEXC error code.
        code: i64,
        /// The resolved error message.
        message: String,
    },
    /// The response status was not success and no error envelope could be parsed.
    #[error("Unexpected HTTP status {status}: {body}")]
    UnexpectedStatus {
        /// The HTTP status code.
        status: u16,
        /// The raw response body.
        body: String,
    },
    /// JSON (de)serialization error.
    #[error("JSON error: {0}")]
    JsonError(String),
    /// Underlying network/transport error.
    #[error("Network error: {0}")]
    NetworkError(String),
    /// Client construction / validation error.
    #[error("Validation error: {0}")]
    ValidationError(String),
}
