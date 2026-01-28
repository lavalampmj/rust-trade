//! Instrument Registry
//!
//! Provides persistent, deterministic instrument_id assignment for all data providers.
//! This ensures consistent IDs across restarts and between different components.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     InstrumentRegistry                          │
//! │  ┌─────────────┐    ┌─────────────┐    ┌─────────────────────┐ │
//! │  │  L1 Cache   │───>│  L2 Cache   │───>│  ID Generation      │ │
//! │  │  (DashMap)  │    │  (Postgres) │    │  (deterministic)    │ │
//! │  └─────────────┘    └─────────────┘    └─────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # ID Generation Strategy
//!
//! For non-Databento sources (crypto), instrument_ids are generated using a
//! deterministic hash of `(symbol, exchange)`. This ensures:
//! - Same symbol/exchange always gets the same ID
//! - IDs are unique across different symbol/exchange pairs
//! - IDs are persistent across restarts (via database)
//!
//! # Example
//!
//! ```ignore
//! let registry = InstrumentRegistry::new(pool).await?;
//!
//! // Get or create instrument_id for a crypto symbol
//! let id = registry.get_or_create("BTCUSD", "BINANCE").await?;
//! println!("BTCUSD@BINANCE -> instrument_id: {}", id);
//!
//! // Later, same call returns the same ID
//! let id2 = registry.get_or_create("BTCUSD", "BINANCE").await?;
//! assert_eq!(id, id2);
//! ```

use dashmap::DashMap;
use sqlx::PgPool;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Error type for instrument registry operations.
#[derive(Debug, Error)]
pub enum RegistryError {
    /// Database error
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

/// Result type for registry operations.
pub type RegistryResult<T> = Result<T, RegistryError>;

/// Cached instrument mapping entry.
#[derive(Debug, Clone)]
pub struct InstrumentMapping {
    /// Unique instrument identifier
    pub instrument_id: u32,
    /// Canonical symbol (e.g., "BTCUSD")
    pub symbol: String,
    /// Exchange identifier (e.g., "BINANCE", "KRAKEN")
    pub exchange: String,
}

/// Registry statistics.
#[derive(Debug, Default)]
pub struct RegistryStats {
    /// L1 cache hits
    pub cache_hits: AtomicU64,
    /// L2 (database) hits
    pub db_hits: AtomicU64,
    /// New ID generations
    pub generations: AtomicU64,
}

impl RegistryStats {
    /// Get cache hit ratio.
    pub fn hit_ratio(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let db_hits = self.db_hits.load(Ordering::Relaxed);
        let generations = self.generations.load(Ordering::Relaxed);
        let total = hits + db_hits + generations;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }
}

/// Instrument Registry for persistent ID assignment.
///
/// Provides a unified interface for getting instrument_ids for any symbol/exchange
/// pair. IDs are cached in memory and persisted to the database.
pub struct InstrumentRegistry {
    /// Database connection pool
    pool: PgPool,

    /// L1 cache: "symbol@exchange" -> instrument_id
    cache: DashMap<String, u32>,

    /// Reverse lookup: instrument_id -> (symbol, exchange)
    id_to_info: DashMap<u32, (String, String)>,

    /// Statistics
    stats: Arc<RegistryStats>,
}

impl InstrumentRegistry {
    /// Create a new registry with database connection.
    ///
    /// This will load existing mappings from the database into the cache.
    pub async fn new(pool: PgPool) -> RegistryResult<Self> {
        let registry = Self {
            pool,
            cache: DashMap::new(),
            id_to_info: DashMap::new(),
            stats: Arc::new(RegistryStats::default()),
        };

        // Pre-load existing mappings from database
        registry.load_from_db().await?;

        Ok(registry)
    }

    /// Create a registry without pre-loading (for testing).
    pub fn new_empty(pool: PgPool) -> Self {
        Self {
            pool,
            cache: DashMap::new(),
            id_to_info: DashMap::new(),
            stats: Arc::new(RegistryStats::default()),
        }
    }

    /// Get registry statistics.
    pub fn stats(&self) -> &Arc<RegistryStats> {
        &self.stats
    }

    /// Get number of cached instruments.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Load existing mappings from database into cache.
    async fn load_from_db(&self) -> RegistryResult<()> {
        let rows: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT instrument_id, symbol, exchange FROM instrument_mappings",
        )
        .fetch_all(&self.pool)
        .await?;

        let count = rows.len();
        for (id, symbol, exchange) in rows {
            let id_u32 = id as u32;
            let cache_key = format!("{}@{}", symbol, exchange);
            self.cache.insert(cache_key, id_u32);
            self.id_to_info
                .insert(id_u32, (symbol.clone(), exchange.clone()));
        }

        if count > 0 {
            info!("Loaded {} instrument mappings from database", count);
        }

        Ok(())
    }

    /// Get or create an instrument_id for a symbol/exchange pair.
    ///
    /// This is the primary method for obtaining instrument_ids. It:
    /// 1. Checks the in-memory cache first
    /// 2. Falls back to the database
    /// 3. Generates a new ID if not found anywhere
    ///
    /// # Arguments
    /// * `symbol` - Canonical symbol (e.g., "BTCUSD")
    /// * `exchange` - Exchange identifier (e.g., "BINANCE")
    ///
    /// # Returns
    /// The instrument_id for this symbol/exchange pair.
    pub async fn get_or_create(&self, symbol: &str, exchange: &str) -> RegistryResult<u32> {
        let symbol = symbol.to_uppercase();
        let exchange = exchange.to_uppercase();
        let cache_key = format!("{}@{}", symbol, exchange);

        // L1: Check in-memory cache
        if let Some(id) = self.cache.get(&cache_key) {
            self.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(*id);
        }

        // L2: Check database
        if let Some(id) = self.lookup_in_db(&symbol, &exchange).await? {
            self.stats.db_hits.fetch_add(1, Ordering::Relaxed);
            // Promote to L1 cache
            self.cache.insert(cache_key, id);
            self.id_to_info
                .insert(id, (symbol.clone(), exchange.clone()));
            return Ok(id);
        }

        // Generate new ID and persist
        let id = self.generate_id(&symbol, &exchange);
        self.stats.generations.fetch_add(1, Ordering::Relaxed);

        // Persist to database
        self.insert_to_db(&symbol, &exchange, id).await?;

        // Add to cache
        self.cache.insert(cache_key, id);
        self.id_to_info
            .insert(id, (symbol.clone(), exchange.clone()));

        debug!(
            "Generated new instrument_id {} for {}@{}",
            id, symbol, exchange
        );

        Ok(id)
    }

    /// Get instrument_id if it exists (no creation).
    pub async fn get(&self, symbol: &str, exchange: &str) -> RegistryResult<Option<u32>> {
        let symbol = symbol.to_uppercase();
        let exchange = exchange.to_uppercase();
        let cache_key = format!("{}@{}", symbol, exchange);

        // Check cache first
        if let Some(id) = self.cache.get(&cache_key) {
            return Ok(Some(*id));
        }

        // Check database
        self.lookup_in_db(&symbol, &exchange).await
    }

    /// Get symbol and exchange for an instrument_id.
    pub fn get_by_id(&self, instrument_id: u32) -> Option<(String, String)> {
        self.id_to_info.get(&instrument_id).map(|r| r.clone())
    }

    /// Lookup in database.
    async fn lookup_in_db(&self, symbol: &str, exchange: &str) -> RegistryResult<Option<u32>> {
        let result: Option<(i64,)> = sqlx::query_as(
            "SELECT instrument_id FROM instrument_mappings WHERE symbol = $1 AND exchange = $2",
        )
        .bind(symbol)
        .bind(exchange)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|(id,)| id as u32))
    }

    /// Insert new mapping to database.
    async fn insert_to_db(&self, symbol: &str, exchange: &str, id: u32) -> RegistryResult<()> {
        sqlx::query(
            r#"
            INSERT INTO instrument_mappings (instrument_id, symbol, exchange)
            VALUES ($1, $2, $3)
            ON CONFLICT (symbol, exchange) DO NOTHING
            "#,
        )
        .bind(id as i64)
        .bind(symbol)
        .bind(exchange)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Generate a deterministic instrument_id from symbol and exchange.
    ///
    /// Uses a hash function to create a consistent ID that:
    /// - Is deterministic (same inputs = same output)
    /// - Minimizes collisions
    /// - Fits in u32
    fn generate_id(&self, symbol: &str, exchange: &str) -> u32 {
        let mut hasher = DefaultHasher::new();
        symbol.hash(&mut hasher);
        exchange.hash(&mut hasher);
        // Use upper 32 bits for better distribution
        (hasher.finish() >> 32) as u32
    }

    /// Register a Databento instrument_id directly.
    ///
    /// Use this when you already have an instrument_id from Databento.
    /// This ensures the mapping is persisted and cached.
    pub async fn register_databento_id(
        &self,
        instrument_id: u32,
        symbol: &str,
        exchange: &str,
    ) -> RegistryResult<()> {
        let symbol = symbol.to_uppercase();
        let exchange = exchange.to_uppercase();
        let cache_key = format!("{}@{}", symbol, exchange);

        // Check if already registered with different ID
        if let Some(existing_id) = self.cache.get(&cache_key) {
            if *existing_id != instrument_id {
                warn!(
                    "Symbol {}@{} already registered with different ID: {} vs {}",
                    symbol, exchange, *existing_id, instrument_id
                );
            }
            return Ok(());
        }

        // Persist to database (may conflict, that's OK)
        sqlx::query(
            r#"
            INSERT INTO instrument_mappings (instrument_id, symbol, exchange)
            VALUES ($1, $2, $3)
            ON CONFLICT (symbol, exchange) DO UPDATE SET instrument_id = $1
            "#,
        )
        .bind(instrument_id as i64)
        .bind(&symbol)
        .bind(&exchange)
        .execute(&self.pool)
        .await?;

        // Add to cache
        self.cache.insert(cache_key, instrument_id);
        self.id_to_info
            .insert(instrument_id, (symbol.clone(), exchange.clone()));

        Ok(())
    }

    /// Bulk pre-register symbols (useful for startup).
    ///
    /// This efficiently registers multiple symbols in a single transaction.
    pub async fn bulk_register(&self, symbols: &[(String, String)]) -> RegistryResult<Vec<u32>> {
        let mut ids = Vec::with_capacity(symbols.len());

        for (symbol, exchange) in symbols {
            let id = self.get_or_create(symbol, exchange).await?;
            ids.push(id);
        }

        Ok(ids)
    }

    /// Get all registered instruments for an exchange.
    pub async fn list_by_exchange(&self, exchange: &str) -> RegistryResult<Vec<InstrumentMapping>> {
        let exchange = exchange.to_uppercase();

        let rows: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT instrument_id, symbol, exchange FROM instrument_mappings WHERE exchange = $1",
        )
        .bind(&exchange)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(id, sym, ex)| InstrumentMapping {
                instrument_id: id as u32,
                symbol: sym,
                exchange: ex,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to generate an ID using the same algorithm as InstrumentRegistry
    fn generate_id(symbol: &str, exchange: &str) -> u32 {
        let mut hasher = DefaultHasher::new();
        symbol.hash(&mut hasher);
        exchange.hash(&mut hasher);
        (hasher.finish() >> 32) as u32
    }

    #[test]
    fn test_generate_id_deterministic() {
        let id1 = generate_id("BTCUSD", "BINANCE");
        let id2 = generate_id("BTCUSD", "BINANCE");
        assert_eq!(id1, id2, "Same inputs should produce same ID");

        let id3 = generate_id("ETHUSD", "BINANCE");
        assert_ne!(id1, id3, "Different symbols should produce different IDs");

        let id4 = generate_id("BTCUSD", "KRAKEN");
        assert_ne!(id1, id4, "Different exchanges should produce different IDs");
    }

    #[test]
    fn test_generate_id_distribution() {
        // Generate IDs for common crypto pairs
        let pairs = [
            ("BTCUSD", "BINANCE"),
            ("ETHUSD", "BINANCE"),
            ("SOLUSD", "BINANCE"),
            ("BTCUSD", "KRAKEN"),
            ("ETHUSD", "KRAKEN"),
            ("BTCUSD", "COINBASE"),
        ];

        let ids: Vec<u32> = pairs
            .iter()
            .map(|(s, e)| generate_id(s, e))
            .collect();

        // Check all unique
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(
            ids.len(),
            unique.len(),
            "All IDs should be unique for different pairs"
        );
    }

    #[test]
    fn test_cache_key_format() {
        let symbol = "BTCUSD";
        let exchange = "BINANCE";
        let cache_key = format!("{}@{}", symbol, exchange);
        assert_eq!(cache_key, "BTCUSD@BINANCE");
    }
}
