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

//! Error types for the KuCoin HTTP client.

use nautilus_network::http::{HttpClientError, StatusCode};
use thiserror::Error;

/// A typed error enumeration for the KuCoin HTTP client.
#[derive(Debug, Error)]
pub enum KuCoinHttpError {
    /// Credentials are missing but the request requires authentication.
    #[error("Missing credentials for authenticated request")]
    MissingCredentials,
    /// Error returned directly by KuCoin (business `code` != `200000`).
    #[error("KuCoin error {error_code}: {message}")]
    KuCoinError { error_code: String, message: String },
    /// Failure during JSON serialization/deserialization.
    #[error("JSON error: {0}")]
    JsonError(String),
    /// Parameter validation error.
    #[error("Parameter validation error: {0}")]
    ValidationError(String),
    /// Request was canceled, typically due to shutdown or disconnect.
    #[error("Request canceled: {0}")]
    Canceled(String),
    /// Wrapping the underlying `HttpClientError` from the network crate.
    #[error("Network error: {0}")]
    HttpClientError(#[from] HttpClientError),
    /// Any unknown HTTP status or unexpected response from KuCoin.
    #[error("Unexpected HTTP status code {status}: {body}")]
    UnexpectedStatus { status: StatusCode, body: String },
}

impl From<String> for KuCoinHttpError {
    fn from(error: String) -> Self {
        Self::ValidationError(error)
    }
}

impl From<serde_json::Error> for KuCoinHttpError {
    fn from(error: serde_json::Error) -> Self {
        Self::JsonError(error.to_string())
    }
}

impl KuCoinHttpError {
    /// Returns whether this error is retryable (transient network / 5xx / 429).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::HttpClientError(_) => true,
            Self::UnexpectedStatus { status, .. } => {
                status.as_u16() >= 500 || status.as_u16() == 429
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(KuCoinHttpError::UnexpectedStatus { status: StatusCode::INTERNAL_SERVER_ERROR, body: String::new() }, true)]
    #[case(KuCoinHttpError::UnexpectedStatus { status: StatusCode::TOO_MANY_REQUESTS, body: String::new() }, true)]
    #[case(KuCoinHttpError::UnexpectedStatus { status: StatusCode::FORBIDDEN, body: String::new() }, false)]
    #[case(KuCoinHttpError::MissingCredentials, false)]
    #[case(KuCoinHttpError::JsonError("bad".to_string()), false)]
    fn test_is_retryable(#[case] error: KuCoinHttpError, #[case] expected: bool) {
        assert_eq!(error.is_retryable(), expected);
    }
}
