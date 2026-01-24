//! Symbol Registry service for centralized symbol management.
//!
//! This module provides a high-performance symbol registry with:
//! - In-memory DashMap cache for concurrent access
//! - Lazy loading from database
//! - Venue and asset class default fallbacks
//! - Cache invalidation mechanism
//!
//! # Example
//!
//! ```ignore
//! use trading_common::instruments::registry::SymbolRegistry;
//! use sqlx::PgPool;
//!
//! let registry = SymbolRegistry::new(pool);
//!
//! // Get a symbol (loads from DB if not cached)
//! let btcusdt = registry.get(&InstrumentId::new("BTCUSDT", "BINANCE")).await?;
//!
//! // Check if market is open
//! if registry.is_market_open(&btcusdt.id)? {
//!     // Trade...
//! }
//! ```

use chrono::Utc;
use dashmap::DashMap;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

use crate::data::SymbolRepository;
use crate::orders::InstrumentId;

use super::{
    AssetClass, MarketStatus, SessionSchedule, SessionState, SymbolDefinition, SymbolStatus,
    TradingSpecs,
};

/// Error types for the symbol registry
#[derive(Debug, Clone, thiserror::Error)]
pub enum RegistryError {
    /// Symbol not found in registry
    #[error("Symbol not found: {0}")]
    NotFound(String),

    /// Database error
    #[error("Database error: {0}")]
    Database(String),

    /// Validation error
    #[error("Validation error: {0}")]
    Validation(String),

    /// Cache error
    #[error("Cache error: {0}")]
    Cache(String),
}

/// Result type for registry operations
pub type RegistryResult<T> = Result<T, RegistryError>;

/// Central registry for all symbol definitions.
///
/// Provides thread-safe access to symbol metadata with caching and lazy loading.
pub struct SymbolRegistry {
    /// In-memory cache of definitions
    cache: DashMap<String, Arc<SymbolDefinition>>,

    /// Database repository
    repository: SymbolRepository,

    /// Default configurations per venue
    venue_defaults: HashMap<String, VenueDefaults>,

    /// Default configurations per asset class
    asset_defaults: HashMap<AssetClass, AssetDefaults>,

    /// Cache statistics
    stats: Arc<RegistryStats>,
}

/// Cache statistics
#[derive(Debug, Default)]
pub struct RegistryStats {
    /// Number of cache hits
    pub cache_hits: std::sync::atomic::AtomicU64,
    /// Number of cache misses (database loads)
    pub cache_misses: std::sync::atomic::AtomicU64,
    /// Number of invalidations
    pub invalidations: std::sync::atomic::AtomicU64,
}

impl RegistryStats {
    /// Get hit ratio
    pub fn hit_ratio(&self) -> f64 {
        let hits = self.cache_hits.load(std::sync::atomic::Ordering::Relaxed);
        let misses = self.cache_misses.load(std::sync::atomic::Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }
}

/// Default configuration for a venue
#[derive(Debug, Clone)]
pub struct VenueDefaults {
    /// Default session schedule
    pub session_schedule: Option<SessionSchedule>,
    /// Default trading specs
    pub trading_specs: Option<TradingSpecs>,
    /// Default MIC code
    pub mic_code: Option<String>,
}

impl Default for VenueDefaults {
    fn default() -> Self {
        Self {
            session_schedule: None,
            trading_specs: None,
            mic_code: None,
        }
    }
}

/// Default configuration for an asset class
#[derive(Debug, Clone)]
pub struct AssetDefaults {
    /// Default session schedule
    pub session_schedule: Option<SessionSchedule>,
    /// Default trading specs
    pub trading_specs: Option<TradingSpecs>,
}

impl Default for AssetDefaults {
    fn default() -> Self {
        Self {
            session_schedule: None,
            trading_specs: None,
        }
    }
}

impl SymbolRegistry {
    /// Create a new symbol registry with database connection
    pub fn new(pool: PgPool) -> Self {
        Self {
            cache: DashMap::new(),
            repository: SymbolRepository::new(pool),
            venue_defaults: Self::default_venue_configs(),
            asset_defaults: Self::default_asset_configs(),
            stats: Arc::new(RegistryStats::default()),
        }
    }

    /// Create with custom defaults
    pub fn with_defaults(
        pool: PgPool,
        venue_defaults: HashMap<String, VenueDefaults>,
        asset_defaults: HashMap<AssetClass, AssetDefaults>,
    ) -> Self {
        Self {
            cache: DashMap::new(),
            repository: SymbolRepository::new(pool),
            venue_defaults,
            asset_defaults,
            stats: Arc::new(RegistryStats::default()),
        }
    }

    /// Get default venue configurations
    fn default_venue_configs() -> HashMap<String, VenueDefaults> {
        let mut defaults = HashMap::new();

        // Binance defaults (24/7 crypto)
        defaults.insert(
            "BINANCE".to_string(),
            VenueDefaults {
                session_schedule: Some(super::session::presets::crypto_24_7()),
                trading_specs: None,
                mic_code: Some("BINC".to_string()),
            },
        );

        // NYSE defaults
        defaults.insert(
            "NYSE".to_string(),
            VenueDefaults {
                session_schedule: Some(super::session::presets::us_equity()),
                trading_specs: None,
                mic_code: Some("XNYS".to_string()),
            },
        );

        // CME Globex defaults
        defaults.insert(
            "GLBX".to_string(),
            VenueDefaults {
                session_schedule: Some(super::session::presets::cme_globex()),
                trading_specs: None,
                mic_code: Some("GLBX".to_string()),
            },
        );

        defaults
    }

    /// Get default asset class configurations
    fn default_asset_configs() -> HashMap<AssetClass, AssetDefaults> {
        let mut defaults = HashMap::new();

        // Crypto defaults (24/7)
        defaults.insert(
            AssetClass::Crypto,
            AssetDefaults {
                session_schedule: Some(super::session::presets::crypto_24_7()),
                trading_specs: None,
            },
        );

        // FX defaults (24/5)
        defaults.insert(
            AssetClass::FX,
            AssetDefaults {
                session_schedule: Some(super::session::presets::forex()),
                trading_specs: None,
            },
        );

        defaults
    }

    /// Get registry statistics
    pub fn stats(&self) -> &Arc<RegistryStats> {
        &self.stats
    }

    /// Get number of cached symbols
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    // =================================================================
    // Symbol Retrieval Operations
    // =================================================================

    /// Get a symbol definition by ID.
    ///
    /// First checks the cache, then loads from database if not found.
    pub async fn get(&self, id: &InstrumentId) -> RegistryResult<Arc<SymbolDefinition>> {
        let cache_key = id.to_string();

        // Check cache first
        if let Some(def) = self.cache.get(&cache_key) {
            self.stats
                .cache_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Ok(def.clone());
        }

        // Load from database
        self.stats
            .cache_misses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let def = self
            .repository
            .get(id)
            .await
            .map_err(|e| RegistryError::Database(e.to_string()))?
            .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;

        let def = Arc::new(def);
        self.cache.insert(cache_key, def.clone());

        debug!("Loaded symbol definition from database: {}", id);
        Ok(def)
    }

    /// Get a symbol definition, returning None if not found.
    pub async fn get_opt(
        &self,
        id: &InstrumentId,
    ) -> RegistryResult<Option<Arc<SymbolDefinition>>> {
        match self.get(id).await {
            Ok(def) => Ok(Some(def)),
            Err(RegistryError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Get multiple symbol definitions by IDs.
    pub async fn get_many(
        &self,
        ids: &[InstrumentId],
    ) -> RegistryResult<Vec<Arc<SymbolDefinition>>> {
        let mut results = Vec::with_capacity(ids.len());
        for id in ids {
            results.push(self.get(id).await?);
        }
        Ok(results)
    }

    /// Get all symbol definitions for a venue.
    pub async fn get_by_venue(&self, venue: &str) -> RegistryResult<Vec<Arc<SymbolDefinition>>> {
        let definitions = self
            .repository
            .list_by_venue(venue)
            .await
            .map_err(|e| RegistryError::Database(e.to_string()))?;

        let results: Vec<Arc<SymbolDefinition>> = definitions
            .into_iter()
            .map(|def| {
                let cache_key = def.id.to_string();
                let arc_def = Arc::new(def);
                self.cache.insert(cache_key, arc_def.clone());
                arc_def
            })
            .collect();

        Ok(results)
    }

    /// Get all active symbol definitions.
    pub async fn get_active(&self) -> RegistryResult<Vec<Arc<SymbolDefinition>>> {
        let definitions = self
            .repository
            .list_by_status(SymbolStatus::Active)
            .await
            .map_err(|e| RegistryError::Database(e.to_string()))?;

        let results: Vec<Arc<SymbolDefinition>> = definitions
            .into_iter()
            .map(|def| {
                let cache_key = def.id.to_string();
                let arc_def = Arc::new(def);
                self.cache.insert(cache_key, arc_def.clone());
                arc_def
            })
            .collect();

        Ok(results)
    }

    /// Search for symbols by pattern.
    pub async fn search(&self, pattern: &str) -> RegistryResult<Vec<Arc<SymbolDefinition>>> {
        let definitions = self
            .repository
            .search(pattern)
            .await
            .map_err(|e| RegistryError::Database(e.to_string()))?;

        let results: Vec<Arc<SymbolDefinition>> = definitions
            .into_iter()
            .map(|def| {
                let cache_key = def.id.to_string();
                let arc_def = Arc::new(def);
                self.cache.insert(cache_key, arc_def.clone());
                arc_def
            })
            .collect();

        Ok(results)
    }

    // =================================================================
    // Symbol Registration Operations
    // =================================================================

    /// Register a new symbol definition.
    pub async fn register(&self, def: SymbolDefinition) -> RegistryResult<()> {
        let cache_key = def.id.to_string();

        self.repository
            .upsert(&def)
            .await
            .map_err(|e| RegistryError::Database(e.to_string()))?;

        let arc_def = Arc::new(def);
        self.cache.insert(cache_key.clone(), arc_def);

        info!("Registered symbol: {}", cache_key);
        Ok(())
    }

    /// Register multiple symbol definitions.
    pub async fn register_many(&self, definitions: Vec<SymbolDefinition>) -> RegistryResult<()> {
        for def in definitions {
            self.register(def).await?;
        }
        Ok(())
    }

    /// Unregister (delete) a symbol definition.
    pub async fn unregister(&self, id: &InstrumentId) -> RegistryResult<bool> {
        let cache_key = id.to_string();

        let deleted = self
            .repository
            .delete(id)
            .await
            .map_err(|e| RegistryError::Database(e.to_string()))?;

        if deleted {
            self.cache.remove(&cache_key);
            info!("Unregistered symbol: {}", cache_key);
        }

        Ok(deleted)
    }

    // =================================================================
    // Cache Management
    // =================================================================

    /// Invalidate a cached symbol definition.
    pub fn invalidate(&self, id: &InstrumentId) {
        let cache_key = id.to_string();
        if self.cache.remove(&cache_key).is_some() {
            self.stats
                .invalidations
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            debug!("Invalidated cached symbol: {}", cache_key);
        }
    }

    /// Invalidate all cached symbol definitions for a venue.
    pub fn invalidate_venue(&self, venue: &str) {
        let keys_to_remove: Vec<String> = self
            .cache
            .iter()
            .filter(|entry| entry.value().id.venue == venue)
            .map(|entry| entry.key().clone())
            .collect();

        for key in keys_to_remove {
            self.cache.remove(&key);
            self.stats
                .invalidations
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        debug!("Invalidated all cached symbols for venue: {}", venue);
    }

    /// Clear the entire cache.
    pub fn clear_cache(&self) {
        let count = self.cache.len();
        self.cache.clear();
        self.stats
            .invalidations
            .fetch_add(count as u64, std::sync::atomic::Ordering::Relaxed);
        info!("Cleared symbol registry cache ({} entries)", count);
    }

    /// Preload all symbols from a venue into cache.
    pub async fn preload_venue(&self, venue: &str) -> RegistryResult<usize> {
        let definitions = self.get_by_venue(venue).await?;
        let count = definitions.len();
        info!("Preloaded {} symbols from venue {}", count, venue);
        Ok(count)
    }

    /// Preload all active symbols into cache.
    pub async fn preload_active(&self) -> RegistryResult<usize> {
        let definitions = self.get_active().await?;
        let count = definitions.len();
        info!("Preloaded {} active symbols", count);
        Ok(count)
    }

    // =================================================================
    // Market State Operations
    // =================================================================

    /// Check if market is open for a symbol.
    pub fn is_market_open(&self, id: &InstrumentId) -> RegistryResult<bool> {
        let cache_key = id.to_string();

        let def = self
            .cache
            .get(&cache_key)
            .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;

        if let Some(schedule) = &def.session_schedule {
            Ok(schedule.is_open(Utc::now()))
        } else {
            // No session schedule = 24/7 market (crypto)
            Ok(true)
        }
    }

    /// Get session state for a symbol.
    pub fn get_session_state(&self, id: &InstrumentId) -> RegistryResult<SessionState> {
        let cache_key = id.to_string();

        let def = self
            .cache
            .get(&cache_key)
            .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;

        if let Some(schedule) = &def.session_schedule {
            Ok(schedule.get_session_state(Utc::now()))
        } else {
            // 24/7 market
            Ok(SessionState {
                status: MarketStatus::Open,
                current_session: None,
                next_change: None,
                reason: None,
            })
        }
    }

    /// Check if symbol is tradeable (active status and market open).
    pub fn is_tradeable(&self, id: &InstrumentId) -> RegistryResult<bool> {
        let cache_key = id.to_string();

        let def = self
            .cache
            .get(&cache_key)
            .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;

        // Check status
        if def.status != SymbolStatus::Active {
            return Ok(false);
        }

        // Check market hours
        self.is_market_open(id)
    }

    // =================================================================
    // Default Fallback Operations
    // =================================================================

    /// Get session schedule with fallbacks.
    ///
    /// Priority: Symbol -> Venue -> Asset Class -> None (24/7)
    pub fn get_session_schedule(
        &self,
        id: &InstrumentId,
    ) -> RegistryResult<Option<SessionSchedule>> {
        let cache_key = id.to_string();

        let def = self
            .cache
            .get(&cache_key)
            .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;

        // Check symbol's own schedule
        if let Some(schedule) = &def.session_schedule {
            return Ok(Some(schedule.clone()));
        }

        // Check venue defaults
        if let Some(venue_default) = self.venue_defaults.get(&def.id.venue) {
            if let Some(schedule) = &venue_default.session_schedule {
                return Ok(Some(schedule.clone()));
            }
        }

        // Check asset class defaults
        if let Some(asset_default) = self.asset_defaults.get(&def.info.asset_class) {
            if let Some(schedule) = &asset_default.session_schedule {
                return Ok(Some(schedule.clone()));
            }
        }

        // No schedule = 24/7
        Ok(None)
    }

    /// Get trading specs with fallbacks.
    pub fn get_trading_specs(&self, id: &InstrumentId) -> RegistryResult<TradingSpecs> {
        let cache_key = id.to_string();

        let def = self
            .cache
            .get(&cache_key)
            .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;

        // Symbol's own specs (always present)
        Ok(def.trading_specs.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // Note: Full integration tests require a test database
    // These tests cover the cache logic and defaults

    #[test]
    fn test_registry_stats() {
        let stats = RegistryStats::default();
        assert_eq!(stats.hit_ratio(), 0.0);

        stats
            .cache_hits
            .fetch_add(7, std::sync::atomic::Ordering::Relaxed);
        stats
            .cache_misses
            .fetch_add(3, std::sync::atomic::Ordering::Relaxed);

        assert!((stats.hit_ratio() - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_default_venue_configs() {
        let defaults = SymbolRegistry::default_venue_configs();

        assert!(defaults.contains_key("BINANCE"));
        assert!(defaults.contains_key("NYSE"));
        assert!(defaults.contains_key("GLBX"));

        // Binance should have 24/7 schedule
        let binance = defaults.get("BINANCE").unwrap();
        assert!(binance.session_schedule.is_some());
    }

    #[test]
    fn test_default_asset_configs() {
        let defaults = SymbolRegistry::default_asset_configs();

        assert!(defaults.contains_key(&AssetClass::Crypto));
        assert!(defaults.contains_key(&AssetClass::FX));

        // Crypto should have 24/7 schedule
        let crypto = defaults.get(&AssetClass::Crypto).unwrap();
        assert!(crypto.session_schedule.is_some());
    }

    // ============================================================
    // CACHE LOGIC TESTS
    // ============================================================

    #[test]
    fn test_cache_insert_and_get() {
        use crate::instruments::{SymbolInfo, VenueConfig};

        // Create a mock registry without database
        let cache: DashMap<String, Arc<SymbolDefinition>> = DashMap::new();

        // Create a test symbol definition
        let def = SymbolDefinition::new(
            "BTCUSDT",
            "BINANCE",
            SymbolInfo::crypto_spot("BTC", "USDT"),
            VenueConfig::binance("BTCUSDT"),
            TradingSpecs::crypto(dec!(0.01), dec!(0.00001), 2, 5),
        );

        let key = "BTCUSDT.BINANCE".to_string();
        cache.insert(key.clone(), Arc::new(def));

        // Verify cache contains the symbol
        assert!(cache.contains_key(&key));

        let retrieved = cache.get(&key).unwrap();
        assert_eq!(retrieved.id.symbol, "BTCUSDT");
        assert_eq!(retrieved.id.venue, "BINANCE");
    }

    #[test]
    fn test_cache_not_found() {
        let cache: DashMap<String, Arc<SymbolDefinition>> = DashMap::new();

        // Non-existent key
        assert!(cache.get("NONEXISTENT.VENUE").is_none());
    }

    #[test]
    fn test_cache_overwrite() {
        use crate::instruments::{SymbolInfo, VenueConfig};

        let cache: DashMap<String, Arc<SymbolDefinition>> = DashMap::new();

        let key = "BTCUSDT.BINANCE".to_string();

        // Insert first version
        let def1 = SymbolDefinition::new(
            "BTCUSDT",
            "BINANCE",
            SymbolInfo::crypto_spot("BTC", "USDT"),
            VenueConfig::binance("BTCUSDT"),
            TradingSpecs::crypto(dec!(0.01), dec!(0.00001), 2, 5),
        );
        cache.insert(key.clone(), Arc::new(def1));

        // Insert second version (overwrites)
        let def2 = SymbolDefinition::new(
            "BTCUSDT",
            "BINANCE",
            SymbolInfo::crypto_spot("BTC", "USDT"),
            VenueConfig::binance("BTCUSDT"),
            TradingSpecs::crypto(dec!(0.02), dec!(0.00001), 2, 5), // Different tick size
        );
        cache.insert(key.clone(), Arc::new(def2));

        // Should have the second version
        let retrieved = cache.get(&key).unwrap();
        assert_eq!(retrieved.trading_specs.min_price_increment, dec!(0.02));
    }

    #[test]
    fn test_cache_remove() {
        use crate::instruments::{SymbolInfo, VenueConfig};

        let cache: DashMap<String, Arc<SymbolDefinition>> = DashMap::new();

        let key = "BTCUSDT.BINANCE".to_string();

        let def = SymbolDefinition::new(
            "BTCUSDT",
            "BINANCE",
            SymbolInfo::crypto_spot("BTC", "USDT"),
            VenueConfig::binance("BTCUSDT"),
            TradingSpecs::crypto(dec!(0.01), dec!(0.00001), 2, 5),
        );
        cache.insert(key.clone(), Arc::new(def));

        // Remove
        let removed = cache.remove(&key);
        assert!(removed.is_some());

        // Should no longer exist
        assert!(cache.get(&key).is_none());

        // Remove again should return None
        assert!(cache.remove(&key).is_none());
    }

    // ============================================================
    // CONCURRENT ACCESS TESTS
    // ============================================================

    #[test]
    fn test_concurrent_cache_reads() {
        use crate::instruments::{SymbolInfo, VenueConfig};
        use std::thread;

        let cache: Arc<DashMap<String, Arc<SymbolDefinition>>> = Arc::new(DashMap::new());

        // Pre-populate cache
        for i in 0..100 {
            let symbol = format!("SYM{}", i);
            let key = format!("{}.BINANCE", symbol);
            let def = SymbolDefinition::new(
                &symbol,
                "BINANCE",
                SymbolInfo::crypto_spot("BTC", "USDT"),
                VenueConfig::binance(&symbol),
                TradingSpecs::crypto(dec!(0.01), dec!(0.00001), 2, 5),
            );
            cache.insert(key, Arc::new(def));
        }

        // Spawn multiple readers
        let handles: Vec<_> = (0..10)
            .map(|thread_id| {
                let cache_clone = Arc::clone(&cache);
                thread::spawn(move || {
                    for i in 0..100 {
                        let key = format!("SYM{}.BINANCE", (i + thread_id) % 100);
                        let result = cache_clone.get(&key);
                        assert!(
                            result.is_some(),
                            "Thread {} failed to get {}",
                            thread_id,
                            key
                        );
                    }
                })
            })
            .collect();

        // Wait for all threads
        for handle in handles {
            handle.join().expect("Thread panicked");
        }
    }

    #[test]
    fn test_concurrent_cache_writes() {
        use crate::instruments::{SymbolInfo, VenueConfig};
        use std::thread;

        let cache: Arc<DashMap<String, Arc<SymbolDefinition>>> = Arc::new(DashMap::new());

        // Spawn multiple writers
        let handles: Vec<_> = (0..10)
            .map(|thread_id| {
                let cache_clone = Arc::clone(&cache);
                thread::spawn(move || {
                    for i in 0..100 {
                        let symbol = format!("T{}S{}", thread_id, i);
                        let key = format!("{}.BINANCE", symbol);
                        let def = SymbolDefinition::new(
                            &symbol,
                            "BINANCE",
                            SymbolInfo::crypto_spot("BTC", "USDT"),
                            VenueConfig::binance(&symbol),
                            TradingSpecs::crypto(dec!(0.01), dec!(0.00001), 2, 5),
                        );
                        cache_clone.insert(key, Arc::new(def));
                    }
                })
            })
            .collect();

        // Wait for all threads
        for handle in handles {
            handle.join().expect("Thread panicked");
        }

        // All symbols should exist
        assert_eq!(cache.len(), 1000); // 10 threads * 100 symbols
    }

    #[test]
    fn test_concurrent_read_write() {
        use crate::instruments::{SymbolInfo, VenueConfig};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::thread;

        let cache: Arc<DashMap<String, Arc<SymbolDefinition>>> = Arc::new(DashMap::new());
        let read_count = Arc::new(AtomicUsize::new(0));
        let write_count = Arc::new(AtomicUsize::new(0));

        // Pre-populate some symbols
        for i in 0..50 {
            let key = format!("SYM{}.BINANCE", i);
            let def = SymbolDefinition::new(
                &format!("SYM{}", i),
                "BINANCE",
                SymbolInfo::crypto_spot("BTC", "USDT"),
                VenueConfig::binance(&format!("SYM{}", i)),
                TradingSpecs::crypto(dec!(0.01), dec!(0.00001), 2, 5),
            );
            cache.insert(key, Arc::new(def));
        }

        let mut handles = vec![];

        // Reader threads
        for _ in 0..5 {
            let cache_clone = Arc::clone(&cache);
            let count_clone = Arc::clone(&read_count);
            handles.push(thread::spawn(move || {
                for i in 0..100 {
                    let key = format!("SYM{}.BINANCE", i % 100);
                    if cache_clone.get(&key).is_some() {
                        count_clone.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }));
        }

        // Writer threads
        for thread_id in 0..5 {
            let cache_clone = Arc::clone(&cache);
            let count_clone = Arc::clone(&write_count);
            handles.push(thread::spawn(move || {
                for i in 50..100 {
                    let symbol = format!("SYM{}", i);
                    let key = format!("{}.BINANCE", symbol);
                    let def = SymbolDefinition::new(
                        &symbol,
                        "BINANCE",
                        SymbolInfo::crypto_spot("BTC", "USDT"),
                        VenueConfig::binance(&symbol),
                        TradingSpecs::crypto(dec!(0.01), dec!(0.00001), 2, 5),
                    );
                    cache_clone.insert(key, Arc::new(def));
                    count_clone.fetch_add(1, Ordering::Relaxed);
                }
            }));
        }

        // Wait for all
        for handle in handles {
            handle.join().expect("Thread panicked");
        }

        // Verify writes happened
        assert_eq!(write_count.load(Ordering::Relaxed), 250); // 5 threads * 50 writes

        // Cache should have all symbols
        assert_eq!(cache.len(), 100);
    }

    #[test]
    fn test_concurrent_invalidate_during_read() {
        use crate::instruments::{SymbolInfo, VenueConfig};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::thread;

        let cache: Arc<DashMap<String, Arc<SymbolDefinition>>> = Arc::new(DashMap::new());
        let invalidation_done = Arc::new(AtomicBool::new(false));

        // Pre-populate cache
        for i in 0..100 {
            let key = format!("SYM{}.BINANCE", i);
            let def = SymbolDefinition::new(
                &format!("SYM{}", i),
                "BINANCE",
                SymbolInfo::crypto_spot("BTC", "USDT"),
                VenueConfig::binance(&format!("SYM{}", i)),
                TradingSpecs::crypto(dec!(0.01), dec!(0.00001), 2, 5),
            );
            cache.insert(key, Arc::new(def));
        }

        let mut handles = vec![];

        // Reader threads - continuously read
        for _ in 0..5 {
            let cache_clone = Arc::clone(&cache);
            let done = Arc::clone(&invalidation_done);
            handles.push(thread::spawn(move || {
                let mut reads = 0;
                while !done.load(Ordering::Relaxed) || reads < 100 {
                    let key = format!("SYM{}.BINANCE", reads % 100);
                    // This should not panic even if entry is being removed
                    let _ = cache_clone.get(&key);
                    reads += 1;
                }
            }));
        }

        // Invalidator thread - removes entries
        {
            let cache_clone = Arc::clone(&cache);
            let done = Arc::clone(&invalidation_done);
            handles.push(thread::spawn(move || {
                for i in 0..50 {
                    let key = format!("SYM{}.BINANCE", i);
                    cache_clone.remove(&key);
                    thread::yield_now(); // Give readers a chance
                }
                done.store(true, Ordering::Relaxed);
            }));
        }

        // All should complete without panic
        for handle in handles {
            handle
                .join()
                .expect("Thread panicked during concurrent access");
        }
    }

    // ============================================================
    // STATS TRACKING TESTS
    // ============================================================

    #[test]
    fn test_stats_atomic_updates() {
        use std::sync::atomic::Ordering;
        use std::thread;

        let stats = Arc::new(RegistryStats::default());

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let stats_clone = Arc::clone(&stats);
                thread::spawn(move || {
                    for _ in 0..1000 {
                        stats_clone.cache_hits.fetch_add(1, Ordering::Relaxed);
                        stats_clone.cache_misses.fetch_add(1, Ordering::Relaxed);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // 10 threads * 1000 iterations each
        assert_eq!(stats.cache_hits.load(Ordering::Relaxed), 10000);
        assert_eq!(stats.cache_misses.load(Ordering::Relaxed), 10000);
        assert!((stats.hit_ratio() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_stats_hit_ratio_edge_cases() {
        let stats = RegistryStats::default();

        // Zero requests = 0.0 ratio
        assert_eq!(stats.hit_ratio(), 0.0);

        // All hits
        stats
            .cache_hits
            .fetch_add(100, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(stats.hit_ratio(), 1.0);

        // All misses
        let stats2 = RegistryStats::default();
        stats2
            .cache_misses
            .fetch_add(100, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(stats2.hit_ratio(), 0.0);
    }

    // ============================================================
    // FALLBACK LOGIC TESTS
    // ============================================================

    #[test]
    fn test_venue_defaults_have_required_fields() {
        let defaults = SymbolRegistry::default_venue_configs();

        // All venues should have session schedule
        for (venue, config) in defaults.iter() {
            // At minimum, trading specs should have sensible defaults
            if let Some(specs) = &config.trading_specs {
                assert!(
                    specs.min_price_increment > dec!(0),
                    "Venue {} has zero tick size",
                    venue
                );
            }

            // Session schedule presence depends on asset class
            // BINANCE (crypto) should have 24/7
            if *venue == "BINANCE" {
                assert!(
                    config.session_schedule.is_some(),
                    "BINANCE should have 24/7 schedule"
                );
            }
        }
    }

    #[test]
    fn test_asset_defaults_have_required_fields() {
        let defaults = SymbolRegistry::default_asset_configs();

        // Crypto should be 24/7
        let crypto = defaults.get(&AssetClass::Crypto).unwrap();
        assert!(crypto.session_schedule.is_some());
        let schedule = crypto.session_schedule.as_ref().unwrap();
        assert!(schedule.is_open(Utc::now())); // Should always be open

        // FX should be 24/5 (closed weekends)
        let fx = defaults.get(&AssetClass::FX).unwrap();
        assert!(fx.session_schedule.is_some());
    }
}
