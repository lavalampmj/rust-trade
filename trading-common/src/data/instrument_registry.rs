//! Thread-safe instrument registry for symbol/exchange to instrument_id mapping.
//!
//! This module provides a runtime registry for converting between symbol/exchange
//! pairs and their corresponding DBN instrument_id values. The registry supports:
//!
//! - Thread-safe read/write access via RwLock
//! - Bidirectional lookup (symbol→id and id→symbol)
//! - Database persistence for restart recovery
//!
//! # Usage
//!
//! ```ignore
//! use trading_common::data::InstrumentRegistry;
//!
//! // Create a shared registry
//! let registry = InstrumentRegistry::new();
//!
//! // Register symbols (thread-safe)
//! let btc_id = registry.register("BTCUSDT", "BINANCE");
//! let eth_id = registry.register("ETHUSDT", "BINANCE");
//!
//! // Lookup by ID
//! if let Some((symbol, exchange)) = registry.lookup(btc_id) {
//!     println!("Found: {}@{}", symbol, exchange);
//! }
//!
//! // Lookup by symbol/exchange
//! if let Some(id) = registry.get_id("BTCUSDT", "BINANCE") {
//!     println!("Instrument ID: {}", id);
//! }
//! ```
//!
//! # Persistence
//!
//! ```ignore
//! use sqlx::PgPool;
//!
//! // Load from database on startup
//! let pool = PgPool::connect(&database_url).await?;
//! let registry = InstrumentRegistry::new();
//! registry.load_from_db(&pool).await?;
//!
//! // Register new symbol (automatically persisted)
//! registry.register_with_persist(&pool, "BTCUSDT", "BINANCE").await?;
//!
//! // Or register in memory only, then batch persist
//! registry.register("ETHUSDT", "BINANCE");
//! registry.save_to_db(&pool).await?;
//! ```

use parking_lot::RwLock;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

use super::dbn_types::symbol_to_instrument_id;

/// A thread-safe registry mapping instrument_id ↔ (symbol, exchange).
///
/// Uses `parking_lot::RwLock` for efficient concurrent read access.
#[derive(Debug, Clone, Default)]
pub struct InstrumentRegistry {
    inner: Arc<RwLock<InstrumentRegistryInner>>,
}

#[derive(Debug, Default)]
struct InstrumentRegistryInner {
    /// Maps instrument_id → (symbol, exchange)
    id_to_symbol: HashMap<u32, (String, String)>,

    /// Maps (symbol, exchange) → instrument_id for reverse lookup
    symbol_to_id: HashMap<(String, String), u32>,
}

impl InstrumentRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry with pre-registered symbols
    pub fn with_symbols(symbols: &[(&str, &str)]) -> Self {
        let registry = Self::new();
        for (symbol, exchange) in symbols {
            registry.register(symbol, exchange);
        }
        registry
    }

    /// Register a symbol/exchange pair and return its instrument_id.
    ///
    /// This is idempotent - registering the same symbol/exchange pair
    /// multiple times returns the same instrument_id.
    pub fn register(&self, symbol: &str, exchange: &str) -> u32 {
        let id = symbol_to_instrument_id(symbol, exchange);
        let mut inner = self.inner.write();

        // Insert into both maps (idempotent)
        inner
            .id_to_symbol
            .insert(id, (symbol.to_string(), exchange.to_string()));
        inner
            .symbol_to_id
            .insert((symbol.to_string(), exchange.to_string()), id);

        id
    }

    /// Register multiple symbol/exchange pairs at once.
    ///
    /// More efficient than calling `register` in a loop because it takes
    /// the write lock once.
    pub fn register_batch(&self, symbols: &[(&str, &str)]) -> Vec<u32> {
        let mut inner = self.inner.write();
        let mut ids = Vec::with_capacity(symbols.len());

        for (symbol, exchange) in symbols {
            let id = symbol_to_instrument_id(symbol, exchange);
            inner
                .id_to_symbol
                .insert(id, (symbol.to_string(), exchange.to_string()));
            inner
                .symbol_to_id
                .insert((symbol.to_string(), exchange.to_string()), id);
            ids.push(id);
        }

        ids
    }

    /// Look up symbol/exchange by instrument_id.
    ///
    /// Returns `None` if the ID is not registered.
    pub fn lookup(&self, instrument_id: u32) -> Option<(String, String)> {
        self.inner
            .read()
            .id_to_symbol
            .get(&instrument_id)
            .cloned()
    }

    /// Get instrument_id by symbol/exchange.
    ///
    /// Returns `None` if the symbol/exchange pair is not registered.
    pub fn get_id(&self, symbol: &str, exchange: &str) -> Option<u32> {
        self.inner
            .read()
            .symbol_to_id
            .get(&(symbol.to_string(), exchange.to_string()))
            .copied()
    }

    /// Get instrument_id by symbol/exchange, registering if not found.
    ///
    /// This is a convenience method that combines lookup and registration.
    pub fn get_or_register(&self, symbol: &str, exchange: &str) -> u32 {
        // Fast path: check if already registered
        if let Some(id) = self.get_id(symbol, exchange) {
            return id;
        }

        // Slow path: register
        self.register(symbol, exchange)
    }

    /// Check if an instrument_id is registered.
    pub fn contains(&self, instrument_id: u32) -> bool {
        self.inner.read().id_to_symbol.contains_key(&instrument_id)
    }

    /// Check if a symbol/exchange pair is registered.
    pub fn contains_symbol(&self, symbol: &str, exchange: &str) -> bool {
        self.inner
            .read()
            .symbol_to_id
            .contains_key(&(symbol.to_string(), exchange.to_string()))
    }

    /// Get the number of registered instruments.
    pub fn len(&self) -> usize {
        self.inner.read().id_to_symbol.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.read().id_to_symbol.is_empty()
    }

    /// Get all registered instrument_ids.
    pub fn all_ids(&self) -> Vec<u32> {
        self.inner.read().id_to_symbol.keys().copied().collect()
    }

    /// Get all registered (symbol, exchange) pairs.
    pub fn all_symbols(&self) -> Vec<(String, String)> {
        self.inner.read().id_to_symbol.values().cloned().collect()
    }

    /// Clear all registered instruments.
    pub fn clear(&self) {
        let mut inner = self.inner.write();
        inner.id_to_symbol.clear();
        inner.symbol_to_id.clear();
    }

    /// Unregister an instrument by ID.
    ///
    /// Returns the (symbol, exchange) pair if it was registered.
    pub fn unregister(&self, instrument_id: u32) -> Option<(String, String)> {
        let mut inner = self.inner.write();

        if let Some((symbol, exchange)) = inner.id_to_symbol.remove(&instrument_id) {
            inner
                .symbol_to_id
                .remove(&(symbol.clone(), exchange.clone()));
            Some((symbol, exchange))
        } else {
            None
        }
    }

    /// Export registry contents for persistence.
    ///
    /// Returns all registered (instrument_id, symbol, exchange) tuples.
    pub fn export(&self) -> Vec<(u32, String, String)> {
        self.inner
            .read()
            .id_to_symbol
            .iter()
            .map(|(id, (symbol, exchange))| (*id, symbol.clone(), exchange.clone()))
            .collect()
    }

    /// Import registry contents from persistence.
    ///
    /// Adds all entries to the registry without clearing existing entries.
    pub fn import(&self, entries: &[(u32, String, String)]) {
        let mut inner = self.inner.write();

        for (id, symbol, exchange) in entries {
            inner
                .id_to_symbol
                .insert(*id, (symbol.clone(), exchange.clone()));
            inner
                .symbol_to_id
                .insert((symbol.clone(), exchange.clone()), *id);
        }
    }

    /// Create a snapshot of the registry.
    ///
    /// Returns a non-thread-safe copy of the internal state for serialization.
    pub fn snapshot(&self) -> HashMap<u32, (String, String)> {
        self.inner.read().id_to_symbol.clone()
    }

    // =========================================================================
    // Database Persistence
    // =========================================================================

    /// Load all instrument mappings from the database.
    ///
    /// Populates the registry with all entries from the `instrument_mappings` table.
    /// This should be called on application startup to restore state.
    ///
    /// # Errors
    /// Returns an error if the database query fails.
    pub async fn load_from_db(&self, pool: &PgPool) -> Result<usize, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT instrument_id, symbol, exchange
            FROM instrument_mappings
            ORDER BY instrument_id
            "#,
        )
        .fetch_all(pool)
        .await?;

        let count = rows.len();
        let mut inner = self.inner.write();

        for row in rows {
            let id: i64 = row.get("instrument_id");
            let symbol: String = row.get("symbol");
            let exchange: String = row.get("exchange");

            let id = id as u32;
            inner
                .id_to_symbol
                .insert(id, (symbol.clone(), exchange.clone()));
            inner.symbol_to_id.insert((symbol, exchange), id);
        }

        info!("Loaded {} instrument mappings from database", count);
        Ok(count)
    }

    /// Save all instrument mappings to the database.
    ///
    /// Persists all entries in the registry to the `instrument_mappings` table.
    /// Uses upsert (ON CONFLICT) to handle existing entries.
    ///
    /// # Errors
    /// Returns an error if the database operation fails.
    pub async fn save_to_db(&self, pool: &PgPool) -> Result<usize, sqlx::Error> {
        let entries = self.export();
        let count = entries.len();

        if count == 0 {
            debug!("No instrument mappings to save");
            return Ok(0);
        }

        // Use a transaction for batch insert
        let mut tx = pool.begin().await?;

        for (id, symbol, exchange) in &entries {
            sqlx::query(
                r#"
                INSERT INTO instrument_mappings (instrument_id, symbol, exchange)
                VALUES ($1, $2, $3)
                ON CONFLICT (instrument_id) DO UPDATE SET
                    symbol = EXCLUDED.symbol,
                    exchange = EXCLUDED.exchange
                "#,
            )
            .bind(*id as i64)
            .bind(symbol)
            .bind(exchange)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        info!("Saved {} instrument mappings to database", count);
        Ok(count)
    }

    /// Register a symbol and persist to database.
    ///
    /// Combines registration with database persistence in a single operation.
    ///
    /// # Errors
    /// Returns an error if the database operation fails.
    pub async fn register_with_persist(
        &self,
        pool: &PgPool,
        symbol: &str,
        exchange: &str,
    ) -> Result<u32, sqlx::Error> {
        let id = self.register(symbol, exchange);

        sqlx::query(
            r#"
            INSERT INTO instrument_mappings (instrument_id, symbol, exchange)
            VALUES ($1, $2, $3)
            ON CONFLICT (instrument_id) DO NOTHING
            "#,
        )
        .bind(id as i64)
        .bind(symbol)
        .bind(exchange)
        .execute(pool)
        .await?;

        debug!(
            "Registered and persisted instrument: {} ({} @ {})",
            id, symbol, exchange
        );
        Ok(id)
    }

    /// Register multiple symbols and persist to database.
    ///
    /// More efficient than calling `register_with_persist` in a loop.
    ///
    /// # Errors
    /// Returns an error if the database operation fails.
    pub async fn register_batch_with_persist(
        &self,
        pool: &PgPool,
        symbols: &[(&str, &str)],
    ) -> Result<Vec<u32>, sqlx::Error> {
        let ids = self.register_batch(symbols);

        if ids.is_empty() {
            return Ok(ids);
        }

        let mut tx = pool.begin().await?;

        for (i, (symbol, exchange)) in symbols.iter().enumerate() {
            sqlx::query(
                r#"
                INSERT INTO instrument_mappings (instrument_id, symbol, exchange)
                VALUES ($1, $2, $3)
                ON CONFLICT (instrument_id) DO NOTHING
                "#,
            )
            .bind(ids[i] as i64)
            .bind(*symbol)
            .bind(*exchange)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        debug!("Registered and persisted {} instruments", ids.len());
        Ok(ids)
    }

    /// Delete an instrument mapping from the database.
    ///
    /// # Errors
    /// Returns an error if the database operation fails.
    pub async fn unregister_with_persist(
        &self,
        pool: &PgPool,
        instrument_id: u32,
    ) -> Result<Option<(String, String)>, sqlx::Error> {
        let removed = self.unregister(instrument_id);

        if removed.is_some() {
            sqlx::query("DELETE FROM instrument_mappings WHERE instrument_id = $1")
                .bind(instrument_id as i64)
                .execute(pool)
                .await?;
        }

        Ok(removed)
    }

    /// Get count of mappings in database (for verification).
    pub async fn db_count(pool: &PgPool) -> Result<i64, sqlx::Error> {
        let row = sqlx::query("SELECT COUNT(*) as count FROM instrument_mappings")
            .fetch_one(pool)
            .await?;
        Ok(row.get("count"))
    }

    /// Sync registry with database.
    ///
    /// Loads from DB, then saves any new entries that were registered in memory.
    /// Useful for ensuring consistency after application restart.
    pub async fn sync_with_db(&self, pool: &PgPool) -> Result<(usize, usize), sqlx::Error> {
        // Get current in-memory entries before loading
        let current_entries = self.export();

        // Load from database (adds to registry)
        let loaded = self.load_from_db(pool).await?;

        // Find entries that were in memory but not in DB
        let mut new_count = 0;
        for (id, symbol, exchange) in &current_entries {
            // Check if this was already in DB (would have been loaded)
            // We need to persist entries that were registered in memory only
            let in_db = sqlx::query("SELECT 1 FROM instrument_mappings WHERE instrument_id = $1")
                .bind(*id as i64)
                .fetch_optional(pool)
                .await?
                .is_some();

            if !in_db {
                sqlx::query(
                    r#"
                    INSERT INTO instrument_mappings (instrument_id, symbol, exchange)
                    VALUES ($1, $2, $3)
                    ON CONFLICT (instrument_id) DO NOTHING
                    "#,
                )
                .bind(*id as i64)
                .bind(symbol)
                .bind(exchange)
                .execute(pool)
                .await?;
                new_count += 1;
            }
        }

        if new_count > 0 {
            info!("Synced {} new instrument mappings to database", new_count);
        }

        Ok((loaded, new_count))
    }

    /// Load from symbol_definitions table (alternative to instrument_mappings).
    ///
    /// This loads instrument_id mappings from the comprehensive symbol_definitions
    /// table, which contains full symbol metadata. Use this when you have
    /// SymbolDefinitions already populated and want to populate the registry.
    ///
    /// # Note
    /// This uses the hash-based instrument_id (recalculated from symbol+venue),
    /// not the instrument_id stored in symbol_definitions, to ensure consistency
    /// with the DBN ID generation scheme.
    pub async fn load_from_symbol_definitions(
        &self,
        pool: &PgPool,
    ) -> Result<usize, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT symbol, venue
            FROM symbol_definitions
            WHERE status = 'ACTIVE'
            ORDER BY venue, symbol
            "#,
        )
        .fetch_all(pool)
        .await?;

        let count = rows.len();
        let mut inner = self.inner.write();

        for row in rows {
            let symbol: String = row.get("symbol");
            let exchange: String = row.get("venue");

            // Generate consistent hash-based ID
            let id = symbol_to_instrument_id(&symbol, &exchange);

            inner
                .id_to_symbol
                .insert(id, (symbol.clone(), exchange.clone()));
            inner.symbol_to_id.insert((symbol, exchange), id);
        }

        info!(
            "Loaded {} instrument mappings from symbol_definitions",
            count
        );
        Ok(count)
    }

    /// Create a resolver function suitable for use with StreamEventDbn::to_stream_event.
    ///
    /// Returns a closure that resolves instrument_id to (symbol, exchange).
    /// This is useful for converting DBN events to legacy TickData.
    ///
    /// # Example
    /// ```ignore
    /// let registry = InstrumentRegistry::new();
    /// registry.register("BTCUSDT", "BINANCE");
    ///
    /// let resolver = registry.resolver();
    /// let event: StreamEventDbn = ...;
    /// let legacy_event = event.to_stream_event(resolver);
    /// ```
    pub fn resolver(&self) -> impl Fn(u32) -> Option<(String, String)> + '_ {
        move |instrument_id| self.lookup(instrument_id)
    }
}

/// Global singleton registry for use across the application.
///
/// Use `get_global_registry()` to access this shared instance.
static GLOBAL_REGISTRY: std::sync::OnceLock<InstrumentRegistry> = std::sync::OnceLock::new();

/// Get the global instrument registry.
///
/// This returns a shared instance that can be used across the application.
/// The registry is lazily initialized on first access.
pub fn get_global_registry() -> &'static InstrumentRegistry {
    GLOBAL_REGISTRY.get_or_init(InstrumentRegistry::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_lookup() {
        let registry = InstrumentRegistry::new();

        let btc_id = registry.register("BTCUSDT", "BINANCE");
        let eth_id = registry.register("ETHUSDT", "BINANCE");

        assert_ne!(btc_id, eth_id);

        let (symbol, exchange) = registry.lookup(btc_id).unwrap();
        assert_eq!(symbol, "BTCUSDT");
        assert_eq!(exchange, "BINANCE");

        let (symbol, exchange) = registry.lookup(eth_id).unwrap();
        assert_eq!(symbol, "ETHUSDT");
        assert_eq!(exchange, "BINANCE");
    }

    #[test]
    fn test_get_id() {
        let registry = InstrumentRegistry::new();

        let btc_id = registry.register("BTCUSDT", "BINANCE");

        assert_eq!(registry.get_id("BTCUSDT", "BINANCE"), Some(btc_id));
        assert_eq!(registry.get_id("ETHUSDT", "BINANCE"), None);
    }

    #[test]
    fn test_idempotent_registration() {
        let registry = InstrumentRegistry::new();

        let id1 = registry.register("BTCUSDT", "BINANCE");
        let id2 = registry.register("BTCUSDT", "BINANCE");

        assert_eq!(id1, id2);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_get_or_register() {
        let registry = InstrumentRegistry::new();

        // First call registers
        let id1 = registry.get_or_register("BTCUSDT", "BINANCE");

        // Second call returns existing
        let id2 = registry.get_or_register("BTCUSDT", "BINANCE");

        assert_eq!(id1, id2);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_register_batch() {
        let registry = InstrumentRegistry::new();

        let symbols = vec![
            ("BTCUSDT", "BINANCE"),
            ("ETHUSDT", "BINANCE"),
            ("SOLUSDT", "BINANCE"),
        ];

        let ids = registry.register_batch(&symbols);

        assert_eq!(ids.len(), 3);
        assert_eq!(registry.len(), 3);

        for (i, (symbol, exchange)) in symbols.iter().enumerate() {
            let (s, e) = registry.lookup(ids[i]).unwrap();
            assert_eq!(&s, symbol);
            assert_eq!(&e, exchange);
        }
    }

    #[test]
    fn test_with_symbols() {
        let symbols = vec![("BTCUSDT", "BINANCE"), ("ETHUSDT", "BINANCE")];

        let registry = InstrumentRegistry::with_symbols(&symbols);

        assert_eq!(registry.len(), 2);
        assert!(registry.contains_symbol("BTCUSDT", "BINANCE"));
        assert!(registry.contains_symbol("ETHUSDT", "BINANCE"));
    }

    #[test]
    fn test_contains() {
        let registry = InstrumentRegistry::new();

        let btc_id = registry.register("BTCUSDT", "BINANCE");

        assert!(registry.contains(btc_id));
        assert!(!registry.contains(99999));

        assert!(registry.contains_symbol("BTCUSDT", "BINANCE"));
        assert!(!registry.contains_symbol("UNKNOWN", "BINANCE"));
    }

    #[test]
    fn test_unregister() {
        let registry = InstrumentRegistry::new();

        let btc_id = registry.register("BTCUSDT", "BINANCE");
        assert_eq!(registry.len(), 1);

        let removed = registry.unregister(btc_id);
        assert_eq!(removed, Some(("BTCUSDT".to_string(), "BINANCE".to_string())));
        assert_eq!(registry.len(), 0);

        // Can't lookup anymore
        assert!(registry.lookup(btc_id).is_none());
        assert!(registry.get_id("BTCUSDT", "BINANCE").is_none());
    }

    #[test]
    fn test_clear() {
        let registry = InstrumentRegistry::new();

        registry.register("BTCUSDT", "BINANCE");
        registry.register("ETHUSDT", "BINANCE");
        assert_eq!(registry.len(), 2);

        registry.clear();
        assert_eq!(registry.len(), 0);
        assert!(registry.is_empty());
    }

    #[test]
    fn test_export_import() {
        let registry1 = InstrumentRegistry::new();
        registry1.register("BTCUSDT", "BINANCE");
        registry1.register("ETHUSDT", "BINANCE");

        let exported = registry1.export();
        assert_eq!(exported.len(), 2);

        let registry2 = InstrumentRegistry::new();
        registry2.import(&exported);

        assert_eq!(registry2.len(), 2);
        assert!(registry2.contains_symbol("BTCUSDT", "BINANCE"));
        assert!(registry2.contains_symbol("ETHUSDT", "BINANCE"));
    }

    #[test]
    fn test_all_ids_and_symbols() {
        let registry = InstrumentRegistry::new();

        let btc_id = registry.register("BTCUSDT", "BINANCE");
        let eth_id = registry.register("ETHUSDT", "BINANCE");

        let ids = registry.all_ids();
        assert!(ids.contains(&btc_id));
        assert!(ids.contains(&eth_id));

        let symbols = registry.all_symbols();
        assert!(symbols.contains(&("BTCUSDT".to_string(), "BINANCE".to_string())));
        assert!(symbols.contains(&("ETHUSDT".to_string(), "BINANCE".to_string())));
    }

    #[test]
    fn test_clone_shares_state() {
        let registry1 = InstrumentRegistry::new();
        let registry2 = registry1.clone();

        let id = registry1.register("BTCUSDT", "BINANCE");

        // Both registries share the same state
        assert!(registry2.contains(id));
        assert_eq!(registry2.len(), 1);
    }

    #[test]
    fn test_global_registry() {
        let registry = get_global_registry();

        // Registering in global registry persists across calls
        let id = registry.register("XBTUSDT", "GLOBAL");

        let registry2 = get_global_registry();
        assert!(registry2.contains(id));
    }

    #[test]
    fn test_thread_safety() {
        use std::thread;

        let registry = InstrumentRegistry::new();
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let registry = registry.clone();
                thread::spawn(move || {
                    let symbol = format!("SYM{}", i);
                    registry.register(&symbol, "TEST");
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(registry.len(), 10);
    }
}
