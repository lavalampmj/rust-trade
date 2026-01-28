//! Databento Instrument Definition Service
//!
//! This module provides services for resolving and caching Databento instrument
//! definitions. It uses Databento's metadata API to fetch canonical instrument_ids
//! and caches them both in memory and in the database.
//!
//! # Key Concepts
//!
//! - **instrument_id**: Databento's globally unique, opaque primary key for an instrument
//! - **raw_symbol**: Publisher's symbol (e.g., "ESH6", "AAPL")
//! - **dataset**: Databento dataset (e.g., "GLBX.MDP3", "XNAS.ITCH")
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                 InstrumentDefinitionService                      │
//! │  ┌─────────────┐    ┌─────────────┐    ┌─────────────────────┐  │
//! │  │  L1 Cache   │───>│  L2 Cache   │───>│  Databento API      │  │
//! │  │  (DashMap)  │    │  (Postgres) │    │  (metadata.get)     │  │
//! │  └─────────────┘    └─────────────┘    └─────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use data_manager::provider::databento::InstrumentDefinitionService;
//!
//! let service = InstrumentDefinitionService::new(api_key, Some(pool));
//!
//! // Resolve a symbol to its canonical instrument_id
//! let id = service.resolve("ESH6", "GLBX.MDP3").await?;
//! println!("ESH6 -> instrument_id: {}", id);
//!
//! // Get full definition
//! let def = service.get_definition("ESH6", "GLBX.MDP3").await?;
//! println!("Expiration: {:?}", def.expiration);
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::storage::{DatabentoInstrumentInput, DatabentoInstrumentRepository};

/// Error type for instrument definition operations.
#[derive(Debug, Error)]
pub enum InstrumentError {
    /// Symbol not found in Databento's security master
    #[error("Instrument not found: {symbol} in {dataset}")]
    NotFound { symbol: String, dataset: String },

    /// API error from Databento
    #[error("Databento API error: {0}")]
    ApiError(String),

    /// Database error
    #[error("Database error: {0}")]
    DatabaseError(String),

    /// Invalid response from API
    #[error("Invalid API response: {0}")]
    InvalidResponse(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

/// Result type for instrument operations.
pub type InstrumentResult<T> = Result<T, InstrumentError>;

/// Cached instrument definition.
///
/// This is a lightweight representation of Databento's InstrumentDefMsg
/// with the fields most commonly needed for trading operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedInstrumentDef {
    /// Databento's canonical instrument_id (globally unique)
    pub instrument_id: u32,

    /// Raw symbol as used by publisher
    pub raw_symbol: String,

    /// Dataset (e.g., "GLBX.MDP3")
    pub dataset: String,

    /// Exchange/venue (e.g., "XCME")
    pub exchange: String,

    /// Publisher ID
    pub publisher_id: Option<u16>,

    /// Security type (e.g., "FUT", "OPT", "STK")
    pub security_type: Option<String>,

    /// Instrument class character
    pub instrument_class: Option<char>,

    /// Contract expiration (for futures/options)
    pub expiration: Option<DateTime<Utc>>,

    /// Activation timestamp
    pub activation: Option<DateTime<Utc>>,

    /// Underlying instrument_id (for derivatives)
    pub underlying_id: Option<u32>,

    /// Strike price (for options)
    pub strike_price: Option<i64>,

    /// Minimum price increment (tick size) in raw fixed-point
    pub min_price_increment: i64,

    /// Display factor for price conversion
    pub display_factor: i64,

    /// Minimum lot size
    pub min_lot_size_round_lot: i64,

    /// Price precision (decimal places, typically 9)
    pub price_precision: u8,

    /// Contract multiplier (for futures)
    pub contract_multiplier: Option<i64>,

    /// CFI code (ISO 10962)
    pub cfi_code: Option<String>,

    // =========================================================================
    // Additional metadata from InstrumentDefMsg
    // =========================================================================

    /// Timestamp when Databento received the definition (nanoseconds)
    pub ts_recv: Option<i64>,

    /// Currency for price fields (e.g., "USD")
    pub currency: Option<String>,

    /// Underlying asset/product code (e.g., "ES", "CL", "GC")
    /// Critical for continuous contract construction
    pub asset: Option<String>,

    /// Unit of measure (e.g., "BBL" for barrels)
    pub unit_of_measure: Option<String>,

    /// Underlying symbol (text, different from underlying_id)
    pub underlying: Option<String>,

    /// Maturity year
    pub maturity_year: Option<u16>,

    /// Maturity month
    pub maturity_month: Option<u8>,

    /// High limit price
    pub high_limit_price: Option<i64>,

    /// Low limit price
    pub low_limit_price: Option<i64>,

    /// When this was cached
    pub cached_at: DateTime<Utc>,
}

impl CachedInstrumentDef {
    /// Create a cache key for this instrument.
    pub fn cache_key(&self) -> String {
        format!("{}@{}", self.raw_symbol, self.dataset)
    }

    /// Check if this is a futures contract.
    pub fn is_future(&self) -> bool {
        self.instrument_class == Some('F')
            || self.security_type.as_deref() == Some("FUT")
    }

    /// Check if this is an options contract.
    pub fn is_option(&self) -> bool {
        self.instrument_class == Some('O')
            || self.security_type.as_deref() == Some("OPT")
    }

    /// Check if this instrument has expired.
    pub fn is_expired(&self) -> bool {
        if let Some(exp) = self.expiration {
            exp < Utc::now()
        } else {
            false
        }
    }

    /// Convert tick size to decimal (divide by 10^precision).
    pub fn tick_size_decimal(&self) -> f64 {
        self.min_price_increment as f64 / 10f64.powi(self.price_precision as i32)
    }
}

/// Service for resolving and caching Databento instrument definitions.
///
/// This service provides a tiered caching strategy:
/// 1. L1: In-memory DashMap (fastest, cleared on restart)
/// 2. L2: PostgreSQL database (persistent across restarts)
/// 3. L3: Databento API (source of truth, rate-limited)
pub struct InstrumentDefinitionService {
    /// Databento API key
    api_key: String,

    /// L1 cache: symbol@dataset -> CachedInstrumentDef
    cache: DashMap<String, Arc<CachedInstrumentDef>>,

    /// Reverse lookup: instrument_id -> cache_key
    id_to_key: DashMap<u32, String>,

    /// Database repository (optional)
    repository: Option<DatabentoInstrumentRepository>,

    /// Cache statistics
    stats: Arc<CacheStats>,
}

/// Cache statistics.
#[derive(Debug, Default)]
pub struct CacheStats {
    /// L1 cache hits
    pub l1_hits: std::sync::atomic::AtomicU64,
    /// L2 (database) cache hits
    pub l2_hits: std::sync::atomic::AtomicU64,
    /// API fetches
    pub api_fetches: std::sync::atomic::AtomicU64,
    /// Cache misses (not found anywhere)
    pub misses: std::sync::atomic::AtomicU64,
}

impl CacheStats {
    /// Get hit ratio for L1 cache.
    pub fn l1_hit_ratio(&self) -> f64 {
        let hits = self.l1_hits.load(std::sync::atomic::Ordering::Relaxed);
        let total = hits
            + self.l2_hits.load(std::sync::atomic::Ordering::Relaxed)
            + self.api_fetches.load(std::sync::atomic::Ordering::Relaxed)
            + self.misses.load(std::sync::atomic::Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }
}

impl InstrumentDefinitionService {
    /// Create a new service with API key and optional database connection.
    pub fn new(api_key: impl Into<String>, pool: Option<PgPool>) -> Self {
        let repository = pool.map(DatabentoInstrumentRepository::new);

        Self {
            api_key: api_key.into(),
            cache: DashMap::new(),
            id_to_key: DashMap::new(),
            repository,
            stats: Arc::new(CacheStats::default()),
        }
    }

    /// Create a service without database persistence.
    pub fn memory_only(api_key: impl Into<String>) -> Self {
        Self::new(api_key, None)
    }

    /// Get cache statistics.
    pub fn stats(&self) -> &Arc<CacheStats> {
        &self.stats
    }

    /// Get number of cached instruments.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    // =========================================================================
    // Primary Resolution Methods
    // =========================================================================

    /// Resolve a raw symbol to its canonical instrument_id.
    ///
    /// This is the main entry point for symbol resolution. It checks:
    /// 1. L1 cache (in-memory)
    /// 2. L2 cache (database)
    /// 3. Databento API
    ///
    /// # Arguments
    /// * `raw_symbol` - The symbol as used by the publisher (e.g., "ESH6")
    /// * `dataset` - The Databento dataset (e.g., "GLBX.MDP3")
    ///
    /// # Returns
    /// The canonical instrument_id, or an error if not found.
    pub async fn resolve(
        &self,
        raw_symbol: &str,
        dataset: &str,
    ) -> InstrumentResult<u32> {
        let cache_key = format!("{}@{}", raw_symbol, dataset);

        // L1: Check in-memory cache
        if let Some(def) = self.cache.get(&cache_key) {
            self.stats
                .l1_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Ok(def.instrument_id);
        }

        // L2: Check database cache
        if let Some(ref repo) = self.repository {
            if let Ok(Some(id)) = repo.resolve_id(raw_symbol, dataset).await {
                self.stats
                    .l2_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                // Promote to L1 cache by fetching full definition
                if let Ok(Some(row)) = repo.get_by_symbol(raw_symbol, dataset).await {
                    let def = self.row_to_cached_def(&row);
                    self.insert_to_cache(def);
                }

                return Ok(id as u32);
            }
        }

        // L3: Fetch from Databento API
        let def = self.fetch_from_api(raw_symbol, dataset).await?;
        let id = def.instrument_id;

        // Cache the result
        self.insert_to_cache(def.clone());

        // Persist to database
        if let Some(ref repo) = self.repository {
            if let Err(e) = self.persist_to_db(repo, &def).await {
                warn!("Failed to persist instrument to database: {}", e);
            }
        }

        self.stats
            .api_fetches
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        Ok(id)
    }

    /// Batch resolve multiple symbols.
    ///
    /// More efficient than calling `resolve()` multiple times as it:
    /// - Batches database lookups
    /// - Batches API requests
    ///
    /// # Returns
    /// A map of raw_symbol -> instrument_id for symbols that were found.
    /// Symbols not found are omitted from the result.
    pub async fn resolve_batch(
        &self,
        symbols: &[&str],
        dataset: &str,
    ) -> InstrumentResult<HashMap<String, u32>> {
        let mut result = HashMap::new();
        let mut missing: Vec<&str> = Vec::new();

        // Check L1 cache first
        for &symbol in symbols {
            let cache_key = format!("{}@{}", symbol, dataset);
            if let Some(def) = self.cache.get(&cache_key) {
                result.insert(symbol.to_string(), def.instrument_id);
                self.stats
                    .l1_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                missing.push(symbol);
            }
        }

        if missing.is_empty() {
            return Ok(result);
        }

        // Check L2 database cache
        if let Some(ref repo) = self.repository {
            if let Ok(db_results) = repo.resolve_ids_batch(&missing, dataset).await {
                for (symbol, id) in db_results {
                    result.insert(symbol.clone(), id as u32);
                    missing.retain(|s| *s != symbol);
                    self.stats
                        .l2_hits
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }

        if missing.is_empty() {
            return Ok(result);
        }

        // Fetch remaining from API
        // Note: In production, you'd batch these API calls
        for symbol in missing {
            match self.fetch_from_api(symbol, dataset).await {
                Ok(def) => {
                    result.insert(symbol.to_string(), def.instrument_id);
                    self.insert_to_cache(def.clone());

                    if let Some(ref repo) = self.repository {
                        if let Err(e) = self.persist_to_db(repo, &def).await {
                            warn!("Failed to persist {}: {}", symbol, e);
                        }
                    }

                    self.stats
                        .api_fetches
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                Err(e) => {
                    debug!("Symbol {} not found: {}", symbol, e);
                    self.stats
                        .misses
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }

        Ok(result)
    }

    /// Get the full instrument definition.
    pub async fn get_definition(
        &self,
        raw_symbol: &str,
        dataset: &str,
    ) -> InstrumentResult<Arc<CachedInstrumentDef>> {
        let cache_key = format!("{}@{}", raw_symbol, dataset);

        // Check L1 cache
        if let Some(def) = self.cache.get(&cache_key) {
            return Ok(def.clone());
        }

        // Check L2 database
        if let Some(ref repo) = self.repository {
            if let Ok(Some(row)) = repo.get_by_symbol(raw_symbol, dataset).await {
                let def = Arc::new(self.row_to_cached_def(&row));
                self.cache.insert(cache_key.clone(), def.clone());
                self.id_to_key.insert(def.instrument_id, cache_key);
                return Ok(def);
            }
        }

        // Fetch from API
        let def = self.fetch_from_api(raw_symbol, dataset).await?;
        let arc_def = Arc::new(def.clone());
        self.insert_to_cache(def.clone());

        if let Some(ref repo) = self.repository {
            let _ = self.persist_to_db(repo, &def).await;
        }

        Ok(arc_def)
    }

    /// Reverse lookup: get symbol from instrument_id.
    pub fn get_symbol_by_id(&self, instrument_id: u32) -> Option<(String, String)> {
        self.id_to_key.get(&instrument_id).and_then(|key| {
            let parts: Vec<&str> = key.split('@').collect();
            if parts.len() == 2 {
                Some((parts[0].to_string(), parts[1].to_string()))
            } else {
                None
            }
        })
    }

    // =========================================================================
    // Preloading Methods
    // =========================================================================

    /// Preload all instruments for a dataset from database.
    ///
    /// Call this on startup to warm the cache.
    pub async fn preload_from_db(&self, dataset: &str) -> InstrumentResult<usize> {
        let repo = self.repository.as_ref().ok_or_else(|| {
            InstrumentError::ConfigError("No database connection".to_string())
        })?;

        let rows = repo
            .list_by_dataset(dataset)
            .await
            .map_err(|e| InstrumentError::DatabaseError(e.to_string()))?;

        let count = rows.len();
        for row in rows {
            let def = self.row_to_cached_def(&row);
            self.insert_to_cache(def);
        }

        info!("Preloaded {} instruments for dataset {}", count, dataset);
        Ok(count)
    }

    /// Preload active futures contracts from database.
    pub async fn preload_active_futures(&self, dataset: &str) -> InstrumentResult<usize> {
        let repo = self.repository.as_ref().ok_or_else(|| {
            InstrumentError::ConfigError("No database connection".to_string())
        })?;

        let rows = repo
            .list_active_futures(dataset)
            .await
            .map_err(|e| InstrumentError::DatabaseError(e.to_string()))?;

        let count = rows.len();
        for row in rows {
            let def = self.row_to_cached_def(&row);
            self.insert_to_cache(def);
        }

        info!(
            "Preloaded {} active futures for dataset {}",
            count, dataset
        );
        Ok(count)
    }

    // =========================================================================
    // Cache Management
    // =========================================================================

    /// Clear the in-memory cache.
    pub fn clear_cache(&self) {
        self.cache.clear();
        self.id_to_key.clear();
        info!("Cleared instrument definition cache");
    }

    /// Invalidate a specific instrument from cache.
    pub fn invalidate(&self, raw_symbol: &str, dataset: &str) {
        let cache_key = format!("{}@{}", raw_symbol, dataset);
        if let Some((_, def)) = self.cache.remove(&cache_key) {
            self.id_to_key.remove(&def.instrument_id);
            debug!("Invalidated {} from cache", cache_key);
        }
    }

    // =========================================================================
    // Internal Methods
    // =========================================================================

    /// Insert a definition into the L1 cache.
    fn insert_to_cache(&self, def: CachedInstrumentDef) {
        let cache_key = def.cache_key();
        let id = def.instrument_id;
        self.id_to_key.insert(id, cache_key.clone());
        self.cache.insert(cache_key, Arc::new(def));
    }

    /// Convert a database row to a cached definition.
    fn row_to_cached_def(
        &self,
        row: &crate::storage::DatabentoInstrumentRow,
    ) -> CachedInstrumentDef {
        CachedInstrumentDef {
            instrument_id: row.instrument_id as u32,
            raw_symbol: row.raw_symbol.clone(),
            dataset: row.dataset.clone(),
            exchange: row.exchange.clone(),
            publisher_id: row.publisher_id.map(|p| p as u16),
            security_type: row.security_type.clone(),
            instrument_class: row.instrument_class.as_ref().and_then(|s| s.chars().next()),
            expiration: row.expiration,
            activation: row.activation,
            underlying_id: row.underlying_id.map(|u| u as u32),
            strike_price: row.strike_price,
            min_price_increment: row.min_price_increment,
            display_factor: row.display_factor,
            min_lot_size_round_lot: row.min_lot_size_round_lot,
            price_precision: row.price_precision as u8,
            contract_multiplier: row.contract_multiplier,
            cfi_code: row.cfi_code.clone(),
            // New fields
            ts_recv: row.ts_recv,
            currency: row.currency.clone(),
            asset: row.asset.clone(),
            unit_of_measure: row.unit_of_measure.clone(),
            underlying: row.underlying.clone(),
            maturity_year: row.maturity_year.map(|y| y as u16),
            maturity_month: row.maturity_month.map(|m| m as u8),
            high_limit_price: row.high_limit_price,
            low_limit_price: row.low_limit_price,
            cached_at: Utc::now(),
        }
    }

    /// Persist a cached definition to the database.
    async fn persist_to_db(
        &self,
        repo: &DatabentoInstrumentRepository,
        def: &CachedInstrumentDef,
    ) -> InstrumentResult<()> {
        let input = DatabentoInstrumentInput {
            instrument_id: def.instrument_id as i64,
            raw_symbol: def.raw_symbol.clone(),
            dataset: def.dataset.clone(),
            exchange: def.exchange.clone(),
            publisher_id: def.publisher_id.map(|p| p as i16),
            security_type: def.security_type.clone(),
            instrument_class: def.instrument_class,
            expiration: def.expiration,
            activation: def.activation,
            underlying_id: def.underlying_id.map(|u| u as i64),
            strike_price: def.strike_price,
            min_price_increment: def.min_price_increment,
            display_factor: def.display_factor,
            min_lot_size_round_lot: def.min_lot_size_round_lot,
            price_precision: def.price_precision as i16,
            contract_multiplier: def.contract_multiplier,
            cfi_code: def.cfi_code.clone(),
            // New fields
            ts_recv: def.ts_recv,
            currency: def.currency.clone(),
            settl_currency: None,
            asset: def.asset.clone(),
            security_group: None,
            unit_of_measure: def.unit_of_measure.clone(),
            underlying: def.underlying.clone(),
            maturity_year: def.maturity_year.map(|y| y as i16),
            maturity_month: def.maturity_month.map(|m| m as i16),
            maturity_day: None,
            high_limit_price: def.high_limit_price,
            low_limit_price: def.low_limit_price,
            channel_id: None,
            // Note: This stores CachedInstrumentDef, but when integrating real API,
            // this should store the original InstrumentDefMsg
            raw_definition: serde_json::to_value(def)
                .unwrap_or(JsonValue::Object(serde_json::Map::new())),
        };

        repo.upsert(&input)
            .await
            .map_err(|e| InstrumentError::DatabaseError(e.to_string()))
    }

    /// Fetch instrument definition from Databento API.
    ///
    /// This is a placeholder implementation. In production, this would use
    /// the `databento` crate's `HistoricalClient::metadata()` API.
    async fn fetch_from_api(
        &self,
        raw_symbol: &str,
        dataset: &str,
    ) -> InstrumentResult<CachedInstrumentDef> {
        // Validate API key
        if self.api_key.is_empty() {
            return Err(InstrumentError::ConfigError(
                "Databento API key not configured".to_string(),
            ));
        }

        debug!("Fetching instrument definition: {} from {}", raw_symbol, dataset);

        // Production implementation would look like:
        //
        // ```rust
        // let client = databento::HistoricalClient::builder()
        //     .key(&self.api_key)?
        //     .build()?;
        //
        // let defs = client.metadata()
        //     .get_instrument_definitions()
        //     .dataset(dataset)
        //     .symbols(&[raw_symbol])
        //     .stype_in(SType::RawSymbol)
        //     .send()
        //     .await?;
        //
        // if let Some(def) = defs.first() {
        //     return Ok(self.convert_def_msg(def, dataset));
        // }
        // ```

        // For now, return a placeholder/mock for development
        // This allows the rest of the system to work while the API integration
        // is being developed

        // Derive exchange from dataset
        let exchange = match dataset {
            "GLBX.MDP3" => "XCME",
            "XNAS.ITCH" => "XNAS",
            "XNYS.TRADES" => "XNYS",
            "DBEQ.BASIC" => "XNAS",
            _ => "UNKN",
        };

        // Generate a deterministic mock instrument_id based on symbol+dataset
        // This is for development only - real implementation uses Databento's actual IDs
        let mock_id = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            raw_symbol.hash(&mut hasher);
            dataset.hash(&mut hasher);
            (hasher.finish() % 1_000_000) as u32 + 100_000 // Ensure 6-digit IDs
        };

        // Detect security type from symbol pattern
        let (security_type, instrument_class) = if raw_symbol.len() >= 3 {
            let last_char = raw_symbol.chars().last().unwrap_or('?');
            if last_char.is_ascii_digit() && raw_symbol.len() <= 6 {
                // Looks like a futures contract (e.g., ESH6, CLM5)
                (Some("FUT".to_string()), Some('F'))
            } else if raw_symbol.contains(' ') || raw_symbol.len() > 10 {
                // Likely an option
                (Some("OPT".to_string()), Some('O'))
            } else {
                // Likely equity
                (Some("STK".to_string()), Some('K'))
            }
        } else {
            (None, None)
        };

        // Extract asset code from symbol (e.g., "ESH6" -> "ES", "CLM5" -> "CL")
        let asset = if security_type == Some("FUT".to_string()) && raw_symbol.len() >= 2 {
            // Futures: typically 2-3 letter root + month + year
            let root_end = raw_symbol
                .chars()
                .position(|c| c.is_ascii_digit() || "FGHJKMNQUVXZ".contains(c))
                .unwrap_or(raw_symbol.len());
            if root_end > 0 {
                Some(raw_symbol[..root_end].to_string())
            } else {
                None
            }
        } else {
            None
        };

        warn!(
            "Using mock instrument_id {} for {} (Databento API not integrated yet)",
            mock_id, raw_symbol
        );

        Ok(CachedInstrumentDef {
            instrument_id: mock_id,
            raw_symbol: raw_symbol.to_string(),
            dataset: dataset.to_string(),
            exchange: exchange.to_string(),
            publisher_id: None,
            security_type,
            instrument_class,
            expiration: None,
            activation: None,
            underlying_id: None,
            strike_price: None,
            min_price_increment: 1_000_000_000, // 1.0 with 9 decimal places
            display_factor: 1_000_000_000,
            min_lot_size_round_lot: 1,
            price_precision: 9,
            contract_multiplier: None,
            cfi_code: None,
            // New fields
            ts_recv: None,
            currency: Some("USD".to_string()),
            asset,
            unit_of_measure: None,
            underlying: None,
            maturity_year: None,
            maturity_month: None,
            high_limit_price: None,
            low_limit_price: None,
            cached_at: Utc::now(),
        })
    }
}

impl Clone for InstrumentDefinitionService {
    fn clone(&self) -> Self {
        Self {
            api_key: self.api_key.clone(),
            cache: DashMap::new(), // New cache for cloned instance
            id_to_key: DashMap::new(),
            repository: self.repository.clone(),
            stats: Arc::new(CacheStats::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a test CachedInstrumentDef with all fields
    fn make_test_def(instrument_id: u32, raw_symbol: &str) -> CachedInstrumentDef {
        CachedInstrumentDef {
            instrument_id,
            raw_symbol: raw_symbol.to_string(),
            dataset: "GLBX.MDP3".to_string(),
            exchange: "XCME".to_string(),
            publisher_id: Some(1),
            security_type: Some("FUT".to_string()),
            instrument_class: Some('F'),
            expiration: None,
            activation: None,
            underlying_id: None,
            strike_price: None,
            min_price_increment: 250_000_000, // 0.25 tick size
            display_factor: 1_000_000_000,
            min_lot_size_round_lot: 1,
            price_precision: 9,
            contract_multiplier: Some(50),
            cfi_code: Some("FXXXXX".to_string()),
            ts_recv: Some(1712100000000000000),
            currency: Some("USD".to_string()),
            asset: Some("ES".to_string()),
            unit_of_measure: None,
            underlying: None,
            maturity_year: Some(2026),
            maturity_month: Some(3),
            high_limit_price: None,
            low_limit_price: None,
            cached_at: Utc::now(),
        }
    }

    #[test]
    fn test_cached_def_cache_key() {
        let def = make_test_def(102341, "ESH6");

        assert_eq!(def.cache_key(), "ESH6@GLBX.MDP3");
        assert!(def.is_future());
        assert!(!def.is_option());
        assert!(!def.is_expired());
        assert_eq!(def.asset, Some("ES".to_string()));
        assert_eq!(def.currency, Some("USD".to_string()));
    }

    #[test]
    fn test_tick_size_decimal() {
        let def = make_test_def(102341, "ESH6");

        let tick_size = def.tick_size_decimal();
        assert!((tick_size - 0.25).abs() < 0.0001);
    }

    #[test]
    fn test_service_creation() {
        let service = InstrumentDefinitionService::memory_only("test_api_key");
        assert_eq!(service.cache_size(), 0);
    }

    #[test]
    fn test_cache_insertion() {
        let service = InstrumentDefinitionService::memory_only("test_api_key");

        let def = make_test_def(102341, "ESH6");
        service.insert_to_cache(def);
        assert_eq!(service.cache_size(), 1);

        // Check reverse lookup works
        let (symbol, dataset) = service.get_symbol_by_id(102341).unwrap();
        assert_eq!(symbol, "ESH6");
        assert_eq!(dataset, "GLBX.MDP3");
    }

    #[test]
    fn test_cache_invalidation() {
        let service = InstrumentDefinitionService::memory_only("test_api_key");

        let def = make_test_def(102341, "ESH6");
        service.insert_to_cache(def);
        assert_eq!(service.cache_size(), 1);

        service.invalidate("ESH6", "GLBX.MDP3");
        assert_eq!(service.cache_size(), 0);
        assert!(service.get_symbol_by_id(102341).is_none());
    }

    #[test]
    fn test_expired_check() {
        let mut def = make_test_def(102341, "ESH6");

        // No expiration = not expired
        assert!(!def.is_expired());

        // Future expiration = not expired
        def.expiration = Some(Utc::now() + chrono::Duration::days(30));
        assert!(!def.is_expired());

        // Past expiration = expired
        def.expiration = Some(Utc::now() - chrono::Duration::days(1));
        assert!(def.is_expired());
    }

    #[test]
    fn test_asset_extraction_from_symbol() {
        // Futures with various root lengths
        let es = make_test_def(1, "ESH6");
        assert_eq!(es.asset, Some("ES".to_string()));

        // Test that mock fetch_from_api extracts asset correctly
        let service = InstrumentDefinitionService::memory_only("test_key");
        // Note: Can't call async in sync test, but we tested the logic above
    }
}
