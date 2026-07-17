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

//! BingX HTTP error types.
//!
//! BingX wraps every REST response in `{ "code": <int>, "msg": <string>, "data": ... }`; a non-zero
//! `code` signals a business error (`handle_errors` in CCXT). We surface those as
//! [`BingXHttpError::Api`] so callers see the venue's code + message rather than a swallowed failure.

use thiserror::Error;

/// Errors that can occur when interacting with the BingX HTTP API.
#[derive(Debug, Error)]
pub enum BingXHttpError {
    /// Missing API credentials for a signed (private) endpoint.
    #[error("Missing credentials for private BingX endpoint")]
    MissingCredentials,
    /// The underlying network/transport failed.
    #[error("BingX network error: {0}")]
    Network(String),
    /// The response body could not be deserialized.
    #[error("BingX JSON error: {0}")]
    Json(String),
    /// BingX returned a non-zero business `code`.
    #[error("BingX API error (code {code}): {msg}")]
    Api {
        /// The BingX business error code.
        code: i64,
        /// The BingX error message.
        msg: String,
    },
    /// A non-2xx HTTP status was returned.
    #[error("BingX HTTP status {status}: {body}")]
    Status {
        /// The HTTP status code.
        status: u16,
        /// The (truncated) response body.
        body: String,
    },
}

impl From<serde_json::Error> for BingXHttpError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e.to_string())
    }
}
