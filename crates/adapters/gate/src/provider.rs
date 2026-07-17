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

//! Instrument provider for the Gate adapter (spot markets).

use std::{collections::HashMap, fmt::Debug};

use async_trait::async_trait;
use nautilus_common::providers::{InstrumentProvider, InstrumentStore};
use nautilus_model::identifiers::InstrumentId;

use crate::http::client::GateHttpClient;

/// Loads and caches Gate spot instruments via the REST API.
pub struct GateInstrumentProvider {
    store: InstrumentStore,
    http_client: GateHttpClient,
}

impl Debug for GateInstrumentProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(GateInstrumentProvider))
            .field("store", &self.store)
            .field("http_client", &self.http_client)
            .finish()
    }
}

impl GateInstrumentProvider {
    /// Creates a new provider with an empty store.
    #[must_use]
    pub fn new(http_client: GateHttpClient) -> Self {
        Self {
            store: InstrumentStore::new(),
            http_client,
        }
    }

    /// Returns a reference to the underlying HTTP client.
    #[must_use]
    pub const fn http_client(&self) -> &GateHttpClient {
        &self.http_client
    }

    async fn fetch_all(&self) -> anyhow::Result<Vec<nautilus_model::instruments::InstrumentAny>> {
        self.http_client
            .request_instruments()
            .await
            .map_err(|e| anyhow::anyhow!("failed to fetch Gate spot instruments: {e}"))
    }
}

#[async_trait(?Send)]
impl InstrumentProvider for GateInstrumentProvider {
    fn store(&self) -> &InstrumentStore {
        &self.store
    }

    fn store_mut(&mut self) -> &mut InstrumentStore {
        &mut self.store
    }

    async fn load_all(&mut self, _filters: Option<&HashMap<String, String>>) -> anyhow::Result<()> {
        let instruments = self.fetch_all().await?;
        self.store.clear();
        self.store.add_bulk(instruments);
        self.store.set_initialized();
        Ok(())
    }

    async fn load(
        &mut self,
        instrument_id: &InstrumentId,
        filters: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        // Gate exposes all spot markets from a single endpoint; load everything then verify.
        if !self.store.is_initialized() {
            self.load_all(filters).await?;
        }
        if self.store.contains(instrument_id) {
            Ok(())
        } else {
            anyhow::bail!("Gate instrument not found: {instrument_id}")
        }
    }
}
