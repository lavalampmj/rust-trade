//! Databento client wrapper
//!
//! This module provides the main Databento client that implements the
//! `DataProvider`, `HistoricalDataProvider`, and `LiveStreamProvider` traits.
//!
//! # Instrument Resolution
//!
//! The client integrates with [`InstrumentDefinitionService`] to resolve symbols
//! to Databento's canonical `instrument_id`. This is critical for:
//!
//! - Disambiguating symbols across venues (same symbol, different exchanges)
//! - Handling symbol reuse across years (ESM4 in 2024 vs ESM4 in 2034)
//! - Efficient database queries using the stable numeric ID
//!
//! # Example
//!
//! ```ignore
//! use data_manager::provider::databento::DatabentoClient;
//! use sqlx::PgPool;
//!
//! let mut client = DatabentoClient::from_api_key(api_key);
//! client.connect().await?;
//!
//! // Optional: enable database caching for instrument definitions
//! client.with_database(pool);
//!
//! // Resolve a symbol to its canonical instrument_id
//! let id = client.resolve_instrument_id("ESH6", "GLBX.MDP3").await?;
//!
//! // Get full instrument metadata
//! let def = client.get_instrument_definition("ESH6", "GLBX.MDP3").await?;
//! println!("Tick size: {}", def.tick_size_decimal());
//! ```

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::config::DatabentoSettings;
use crate::provider::{
    ConnectionStatus, DataAvailability, DataProvider, DataType, HistoricalDataProvider,
    HistoricalRequest, LiveStreamProvider, LiveSubscription, ProviderError, ProviderResult,
    StreamCallback, StreamEvent, SubscriptionStatus, VenueCapabilities, VenueConnection,
    VenueConnectionStatus, VenueInfo,
};
use crate::schema::NormalizedOHLC;
use crate::symbol::SymbolSpec;
use trading_common::data::types::TickData;

use super::normalizer::DatabentoNormalizer;
use super::{CachedInstrumentDef, CacheStats, InstrumentDefinitionService, InstrumentError};

/// Databento data provider
///
/// Provides access to both historical and live market data from Databento.
/// Integrates with [`InstrumentDefinitionService`] for canonical symbol resolution.
#[allow(dead_code)] // Fields used for future implementation
pub struct DatabentoClient {
    /// Venue information
    info: VenueInfo,
    /// Databento API key
    api_key: String,
    /// Default dataset
    default_dataset: String,
    /// Connection status
    connected: bool,
    /// Subscription status
    subscription_status: SubscriptionStatus,
    /// Normalizer for converting DBN messages
    normalizer: DatabentoNormalizer,
    /// Instrument definition service for symbol resolution
    instrument_service: InstrumentDefinitionService,
}

impl DatabentoClient {
    /// Create a new Databento client
    pub fn new(settings: &DatabentoSettings) -> Self {
        let info = VenueInfo::data_provider("databento", "Databento")
            .with_version("0.18")
            .with_exchanges(vec![
                "CME".to_string(),
                "CBOT".to_string(),
                "COMEX".to_string(),
                "NYMEX".to_string(),
                "NYSE".to_string(),
                "NASDAQ".to_string(),
                "BATS".to_string(),
                "IEX".to_string(),
            ])
            .with_capabilities(VenueCapabilities {
                supports_live_streaming: true,
                supports_historical: true,
                supports_quotes: true,
                supports_orderbook: true,
                supports_mbo: true,
                ..VenueCapabilities::default()
            });

        // Initialize instrument service without database (memory-only)
        let instrument_service = InstrumentDefinitionService::memory_only(&settings.api_key);

        Self {
            info,
            api_key: settings.api_key.clone(),
            default_dataset: settings.default_dataset.clone(),
            connected: false,
            subscription_status: SubscriptionStatus::default(),
            normalizer: DatabentoNormalizer::new(),
            instrument_service,
        }
    }

    /// Create from API key directly
    pub fn from_api_key(api_key: String) -> Self {
        let settings = DatabentoSettings {
            api_key,
            default_dataset: "GLBX.MDP3".to_string(),
            default_lookback_days: 30,
            reconnection: Default::default(),
        };
        Self::new(&settings)
    }

    /// Enable database caching for instrument definitions.
    ///
    /// When a database pool is provided, resolved instrument definitions will be
    /// persisted to PostgreSQL for faster lookup on subsequent runs.
    pub fn with_database(&mut self, pool: PgPool) {
        self.instrument_service = InstrumentDefinitionService::new(&self.api_key, Some(pool));
        info!("Enabled database caching for instrument definitions");
    }

    /// Get the dataset for a request
    fn get_dataset<'a>(&'a self, request_dataset: Option<&'a str>) -> &'a str {
        request_dataset.unwrap_or(&self.default_dataset)
    }

    // =========================================================================
    // Instrument Resolution Methods
    // =========================================================================

    /// Resolve a symbol to its canonical Databento instrument_id.
    ///
    /// This looks up the globally unique, opaque identifier that Databento assigns
    /// to every instrument in their security master. The ID is:
    /// - Stable forever (never changes for a given instrument)
    /// - Unique across all datasets (no collisions between venues)
    /// - Never reused (even after an instrument delists)
    ///
    /// # Arguments
    /// * `raw_symbol` - The publisher's symbol (e.g., "ESH6", "AAPL")
    /// * `dataset` - The Databento dataset (e.g., "GLBX.MDP3"). If None, uses default.
    ///
    /// # Returns
    /// The canonical instrument_id, or an error if not found.
    pub async fn resolve_instrument_id(
        &self,
        raw_symbol: &str,
        dataset: Option<&str>,
    ) -> ProviderResult<u32> {
        let ds = self.get_dataset(dataset);
        self.instrument_service
            .resolve(raw_symbol, ds)
            .await
            .map_err(|e| match e {
                InstrumentError::NotFound { symbol, dataset } => {
                    ProviderError::SymbolNotFound(format!("{} in {}", symbol, dataset))
                }
                InstrumentError::ApiError(msg) => ProviderError::Request(msg),
                InstrumentError::DatabaseError(msg) => ProviderError::Internal(msg),
                InstrumentError::InvalidResponse(msg) => ProviderError::Parse(msg),
                InstrumentError::ConfigError(msg) => ProviderError::Internal(msg),
            })
    }

    /// Batch resolve multiple symbols to their canonical instrument_ids.
    ///
    /// More efficient than calling `resolve_instrument_id()` multiple times as it
    /// batches database lookups and API requests.
    ///
    /// # Returns
    /// A map of raw_symbol -> instrument_id for symbols that were found.
    /// Symbols not found are omitted from the result.
    pub async fn resolve_instrument_ids(
        &self,
        raw_symbols: &[&str],
        dataset: Option<&str>,
    ) -> ProviderResult<std::collections::HashMap<String, u32>> {
        let ds = self.get_dataset(dataset);
        self.instrument_service
            .resolve_batch(raw_symbols, ds)
            .await
            .map_err(|e| ProviderError::Internal(e.to_string()))
    }

    /// Get the full instrument definition for a symbol.
    ///
    /// Returns cached metadata including:
    /// - `instrument_id`: Canonical ID
    /// - `expiration`: Contract expiration (for futures/options)
    /// - `min_price_increment`: Tick size in fixed-point
    /// - `contract_multiplier`: For futures contracts
    pub async fn get_instrument_definition(
        &self,
        raw_symbol: &str,
        dataset: Option<&str>,
    ) -> ProviderResult<Arc<CachedInstrumentDef>> {
        let ds = self.get_dataset(dataset);
        self.instrument_service
            .get_definition(raw_symbol, ds)
            .await
            .map_err(|e| ProviderError::Internal(e.to_string()))
    }

    /// Reverse lookup: get symbol and dataset from instrument_id.
    ///
    /// Only works for instruments that are currently cached in memory.
    pub fn get_symbol_by_instrument_id(&self, instrument_id: u32) -> Option<(String, String)> {
        self.instrument_service.get_symbol_by_id(instrument_id)
    }

    /// Preload instrument definitions for a dataset from database.
    ///
    /// Call this on startup to warm the in-memory cache.
    pub async fn preload_instruments(&self, dataset: Option<&str>) -> ProviderResult<usize> {
        let ds = self.get_dataset(dataset);
        self.instrument_service
            .preload_from_db(ds)
            .await
            .map_err(|e| ProviderError::Internal(e.to_string()))
    }

    /// Preload active futures contracts from database.
    pub async fn preload_active_futures(&self, dataset: Option<&str>) -> ProviderResult<usize> {
        let ds = self.get_dataset(dataset);
        self.instrument_service
            .preload_active_futures(ds)
            .await
            .map_err(|e| ProviderError::Internal(e.to_string()))
    }

    /// Get instrument definition cache statistics.
    pub fn instrument_cache_stats(&self) -> &Arc<CacheStats> {
        self.instrument_service.stats()
    }

    /// Get number of cached instrument definitions.
    pub fn instrument_cache_size(&self) -> usize {
        self.instrument_service.cache_size()
    }

    /// Clear the instrument definition cache.
    pub fn clear_instrument_cache(&self) {
        self.instrument_service.clear_cache();
    }
}

#[async_trait]
#[async_trait]
impl VenueConnection for DatabentoClient {
    fn info(&self) -> &VenueInfo {
        &self.info
    }

    async fn connect(&mut self) -> trading_common::venue::VenueResult<()> {
        if self.connected {
            return Ok(());
        }

        // Validate API key by making a simple request
        // In production, we'd make an actual validation call to Databento
        if self.api_key.is_empty() {
            return Err(trading_common::venue::VenueError::Authentication(
                "Databento API key not configured".to_string(),
            ));
        }

        info!("Connected to Databento");
        self.connected = true;
        self.subscription_status.connection = ConnectionStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> trading_common::venue::VenueResult<()> {
        self.connected = false;
        self.subscription_status.connection = ConnectionStatus::Disconnected;
        info!("Disconnected from Databento");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn connection_status(&self) -> VenueConnectionStatus {
        if self.connected {
            VenueConnectionStatus::Connected
        } else {
            VenueConnectionStatus::Disconnected
        }
    }
}

#[async_trait]
impl DataProvider for DatabentoClient {
    async fn discover_symbols(&self, exchange: Option<&str>) -> ProviderResult<Vec<SymbolSpec>> {
        if !self.connected {
            return Err(ProviderError::NotConnected);
        }

        // In production, we'd query Databento's metadata API
        // For now, return a stub list of common symbols
        let symbols = match exchange {
            Some("CME") | Some("GLBX") => vec![
                SymbolSpec::new("ES", "CME"),
                SymbolSpec::new("NQ", "CME"),
                SymbolSpec::new("CL", "CME"),
                SymbolSpec::new("GC", "CME"),
                SymbolSpec::new("ZN", "CME"),
                SymbolSpec::new("ZB", "CME"),
            ],
            Some("NASDAQ") | Some("XNAS") => vec![
                SymbolSpec::new("AAPL", "NASDAQ"),
                SymbolSpec::new("MSFT", "NASDAQ"),
                SymbolSpec::new("GOOGL", "NASDAQ"),
                SymbolSpec::new("AMZN", "NASDAQ"),
            ],
            Some("NYSE") | Some("XNYS") => vec![
                SymbolSpec::new("SPY", "NYSE"),
                SymbolSpec::new("QQQ", "NYSE"),
                SymbolSpec::new("IWM", "NYSE"),
            ],
            None => {
                // Return symbols from all supported exchanges
                let mut all = vec![];
                all.extend(self.discover_symbols(Some("CME")).await?);
                all.extend(self.discover_symbols(Some("NASDAQ")).await?);
                all.extend(self.discover_symbols(Some("NYSE")).await?);
                all
            }
            Some(ex) => {
                warn!("Unknown exchange: {}", ex);
                vec![]
            }
        };

        Ok(symbols)
    }
}

#[async_trait]
impl HistoricalDataProvider for DatabentoClient {
    async fn fetch_ticks(
        &self,
        request: &HistoricalRequest,
    ) -> ProviderResult<Box<dyn Iterator<Item = ProviderResult<TickData>> + Send>> {
        if !self.connected {
            return Err(ProviderError::NotConnected);
        }

        let dataset = self.get_dataset(request.dataset.as_deref());

        // Resolve symbols to canonical instrument_ids before fetching
        // This ensures we're using Databento's stable identifiers
        let raw_symbols: Vec<&str> = request
            .symbols
            .iter()
            .map(|s| s.symbol.as_str())
            .collect();

        let resolved_ids = self
            .instrument_service
            .resolve_batch(&raw_symbols, dataset)
            .await
            .map_err(|e| ProviderError::Internal(e.to_string()))?;

        // Log which symbols were resolved
        for symbol in &raw_symbols {
            if let Some(id) = resolved_ids.get(*symbol) {
                debug!("Resolved {} -> instrument_id {}", symbol, id);
            } else {
                warn!("Symbol {} not found in Databento metadata", symbol);
            }
        }

        debug!(
            "Fetching ticks from Databento: dataset={}, symbols={:?} ({} resolved), start={}, end={}",
            dataset, request.symbols, resolved_ids.len(), request.start, request.end
        );

        // In production, this would use the databento crate to fetch data
        // Now that we have resolved instrument_ids, we can use them for more
        // efficient queries and proper symbol mapping in the response.
        //
        // Example of what the real implementation would look like:
        // ```
        // let client = databento::HistoricalClient::builder()
        //     .key(&self.api_key)?
        //     .build()?;
        //
        // // Use resolved instrument_ids for precise matching
        // let decoder = client.timeseries()
        //     .get_range()
        //     .dataset(dataset)
        //     .symbols(&raw_symbols)  // Still pass raw symbols, API resolves internally
        //     .schema(dbn::Schema::Trades)
        //     .stype_in(dbn::SType::RawSymbol)
        //     .start(request.start.format("%Y-%m-%dT%H:%M:%S%.fZ").to_string())
        //     .end(request.end.format("%Y-%m-%dT%H:%M:%S%.fZ").to_string())
        //     .send()
        //     .await?;
        //
        // // Create a lookup map from instrument_id to symbol
        // let id_to_symbol: HashMap<u32, String> = resolved_ids
        //     .iter()
        //     .map(|(sym, id)| (*id, sym.clone()))
        //     .collect();
        //
        // let normalizer = self.normalizer.clone();
        // let iter = decoder.decode().filter_map(move |msg| {
        //     if let Ok(trade) = msg.get::<dbn::TradeMsg>() {
        //         // Use instrument_id from trade message to look up symbol
        //         let symbol = id_to_symbol.get(&trade.hd.instrument_id)?;
        //         Some(normalizer.normalize_trade(trade, symbol, dataset))
        //     } else {
        //         None
        //     }
        // });
        // ```

        Ok(Box::new(std::iter::empty()))
    }

    async fn fetch_ohlc(&self, request: &HistoricalRequest) -> ProviderResult<Vec<NormalizedOHLC>> {
        if !self.connected {
            return Err(ProviderError::NotConnected);
        }

        let dataset = self.get_dataset(request.dataset.as_deref());
        debug!(
            "Fetching OHLC from Databento: dataset={}, symbols={:?}",
            dataset, request.symbols
        );

        // Placeholder - would use databento crate in production
        Ok(vec![])
    }

    async fn check_availability(
        &self,
        _symbol: &SymbolSpec,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> ProviderResult<DataAvailability> {
        if !self.connected {
            return Err(ProviderError::NotConnected);
        }

        // In production, query Databento's metadata API
        // For now, return a placeholder
        Ok(DataAvailability {
            available: true,
            actual_start: Some(start),
            actual_end: Some(end),
            estimated_records: None,
        })
    }
}

#[async_trait]
impl LiveStreamProvider for DatabentoClient {
    async fn subscribe(
        &mut self,
        subscription: LiveSubscription,
        callback: StreamCallback,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> ProviderResult<()> {
        if !self.connected {
            return Err(ProviderError::NotConnected);
        }

        let dataset = self.get_dataset(subscription.dataset.as_deref());

        // Resolve symbols to canonical instrument_ids before subscribing
        // This allows us to build a reverse lookup map for incoming messages
        let raw_symbols: Vec<&str> = subscription
            .symbols
            .iter()
            .map(|s| s.symbol.as_str())
            .collect();

        let resolved_ids = self
            .instrument_service
            .resolve_batch(&raw_symbols, dataset)
            .await
            .map_err(|e| ProviderError::Internal(e.to_string()))?;

        // Build reverse lookup: instrument_id -> symbol
        let _id_to_symbol: std::collections::HashMap<u32, String> = resolved_ids
            .iter()
            .map(|(sym, id): (&String, &u32)| (*id, sym.clone()))
            .collect();

        info!(
            "Subscribing to live data: dataset={}, symbols={:?} ({} resolved)",
            dataset,
            subscription.symbols,
            resolved_ids.len()
        );

        self.subscription_status.symbols = subscription.symbols.clone();

        // In production, this would use databento::LiveClient
        // The id_to_symbol map is used to translate instrument_id in incoming
        // messages back to the original symbol for consistency.
        //
        // Example:
        // ```
        // let mut client = databento::LiveClient::builder()
        //     .key(&self.api_key)?
        //     .dataset(dataset)?
        //     .build()
        //     .await?;
        //
        // client.subscribe(
        //     &raw_symbols,
        //     dbn::Schema::Trades,
        //     dbn::SType::RawSymbol,
        // ).await?;
        //
        // client.start().await?;
        //
        // loop {
        //     tokio::select! {
        //         _ = shutdown_rx.recv() => break,
        //         msg = client.next_record() => {
        //             if let Some(record) = msg {
        //                 // Use instrument_id from message to look up symbol
        //                 let instrument_id = record.hd.instrument_id;
        //                 let symbol = id_to_symbol.get(&instrument_id)
        //                     .cloned()
        //                     .unwrap_or_else(|| format!("UNKNOWN_{}", instrument_id));
        //
        //                 let tick = self.normalizer.normalize_trade(&record, &symbol, dataset)?;
        //                 callback(StreamEvent::Tick(tick));
        //                 self.subscription_status.messages_received += 1;
        //                 self.subscription_status.last_message = Some(Utc::now());
        //             }
        //         }
        //     }
        // }
        // ```

        // Notify that we're connected
        callback(StreamEvent::Status(ConnectionStatus::Connected));

        // Wait for shutdown signal (placeholder)
        let _ = shutdown_rx.recv().await;

        Ok(())
    }

    async fn unsubscribe(&mut self, symbols: &[SymbolSpec]) -> ProviderResult<()> {
        self.subscription_status
            .symbols
            .retain(|s| !symbols.contains(s));
        info!("Unsubscribed from {:?}", symbols);
        Ok(())
    }

    fn subscription_status(&self) -> SubscriptionStatus {
        self.subscription_status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let settings = DatabentoSettings {
            api_key: "test_key".to_string(),
            default_dataset: "GLBX.MDP3".to_string(),
            default_lookback_days: 30,
            reconnection: Default::default(),
        };
        let client = DatabentoClient::new(&settings);
        assert_eq!(client.info().name, "databento");
        assert!(!client.is_connected());
        assert_eq!(client.instrument_cache_size(), 0);
    }

    #[tokio::test]
    async fn test_connect_without_key() {
        let settings = DatabentoSettings {
            api_key: String::new(),
            default_dataset: "GLBX.MDP3".to_string(),
            default_lookback_days: 30,
            reconnection: Default::default(),
        };
        let mut client = DatabentoClient::new(&settings);
        let result = client.connect().await;
        assert!(result.is_err());
    }

    #[test]
    fn test_from_api_key() {
        let client = DatabentoClient::from_api_key("my_api_key".to_string());
        assert_eq!(client.info().name, "databento");
        assert_eq!(client.default_dataset, "GLBX.MDP3");
    }

    #[tokio::test]
    async fn test_resolve_instrument_id() {
        let settings = DatabentoSettings {
            api_key: "test_key".to_string(),
            default_dataset: "GLBX.MDP3".to_string(),
            default_lookback_days: 30,
            reconnection: Default::default(),
        };
        let mut client = DatabentoClient::new(&settings);
        client.connect().await.unwrap();

        // Should resolve using mock implementation (since API not integrated yet)
        let result = client
            .resolve_instrument_id("ESH6", Some("GLBX.MDP3"))
            .await;
        assert!(result.is_ok());

        let id = result.unwrap();
        assert!(id >= 100_000); // Mock IDs are 6 digits

        // Cache should now have one entry
        assert_eq!(client.instrument_cache_size(), 1);

        // Reverse lookup should work
        let (symbol, dataset) = client.get_symbol_by_instrument_id(id).unwrap();
        assert_eq!(symbol, "ESH6");
        assert_eq!(dataset, "GLBX.MDP3");
    }

    #[tokio::test]
    async fn test_resolve_batch() {
        let settings = DatabentoSettings {
            api_key: "test_key".to_string(),
            default_dataset: "GLBX.MDP3".to_string(),
            default_lookback_days: 30,
            reconnection: Default::default(),
        };
        let mut client = DatabentoClient::new(&settings);
        client.connect().await.unwrap();

        let symbols = vec!["ESH6", "CLM5", "GCZ5"];
        let result = client
            .resolve_instrument_ids(&symbols, Some("GLBX.MDP3"))
            .await;

        assert!(result.is_ok());
        let resolved = result.unwrap();
        assert_eq!(resolved.len(), 3);
        assert!(resolved.contains_key("ESH6"));
        assert!(resolved.contains_key("CLM5"));
        assert!(resolved.contains_key("GCZ5"));

        // All IDs should be different
        let ids: std::collections::HashSet<_> = resolved.values().collect();
        assert_eq!(ids.len(), 3);
    }

    #[tokio::test]
    async fn test_get_instrument_definition() {
        let settings = DatabentoSettings {
            api_key: "test_key".to_string(),
            default_dataset: "GLBX.MDP3".to_string(),
            default_lookback_days: 30,
            reconnection: Default::default(),
        };
        let mut client = DatabentoClient::new(&settings);
        client.connect().await.unwrap();

        let def = client
            .get_instrument_definition("ESH6", Some("GLBX.MDP3"))
            .await
            .unwrap();

        assert_eq!(def.raw_symbol, "ESH6");
        assert_eq!(def.dataset, "GLBX.MDP3");
        assert!(def.is_future());
    }

    #[test]
    fn test_clear_instrument_cache() {
        let client = DatabentoClient::from_api_key("test_key".to_string());

        // Cache starts empty
        assert_eq!(client.instrument_cache_size(), 0);

        // Clear should work even on empty cache
        client.clear_instrument_cache();
        assert_eq!(client.instrument_cache_size(), 0);
    }
}
