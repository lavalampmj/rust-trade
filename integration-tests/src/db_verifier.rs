//! Database verification for integration tests
//!
//! This module provides functionality to verify that tick data has been
//! correctly persisted to TimescaleDB during integration tests.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info};

/// Database verification errors
#[derive(Error, Debug)]
pub enum DbVerifierError {
    #[error("Database connection error: {0}")]
    Connection(String),

    #[error("Query error: {0}")]
    Query(#[from] sqlx::Error),

    #[error("Configuration error: {0}")]
    Configuration(String),
}

pub type DbVerifierResult<T> = Result<T, DbVerifierError>;

/// Statistics about persisted ticks
#[derive(Debug, Clone)]
pub struct PersistedTickStats {
    /// Total number of ticks persisted
    pub total_count: u64,
    /// Per-symbol tick counts
    pub symbol_counts: HashMap<String, u64>,
    /// Earliest tick timestamp
    pub earliest_time: Option<DateTime<Utc>>,
    /// Latest tick timestamp
    pub latest_time: Option<DateTime<Utc>>,
    /// Number of distinct symbols
    pub symbol_count: u64,
}

impl Default for PersistedTickStats {
    fn default() -> Self {
        Self {
            total_count: 0,
            symbol_counts: HashMap::new(),
            earliest_time: None,
            latest_time: None,
            symbol_count: 0,
        }
    }
}

/// Database verifier for checking tick persistence
pub struct DbVerifier {
    pool: PgPool,
}

impl DbVerifier {
    /// Create a new database verifier from a connection URL
    pub async fn new(database_url: &str) -> DbVerifierResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .min_connections(1)
            .acquire_timeout(Duration::from_secs(30))
            .connect(database_url)
            .await
            .map_err(|e| DbVerifierError::Connection(e.to_string()))?;

        Ok(Self { pool })
    }

    /// Create a verifier from an existing pool
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get the database pool reference
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Count all test ticks (symbols matching TEST%)
    pub async fn count_test_ticks(&self) -> DbVerifierResult<u64> {
        let row = sqlx::query(
            r#"
            SELECT COUNT(*) as count
            FROM market_ticks
            WHERE symbol LIKE 'TEST%'
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(row.get::<i64, _>("count") as u64)
    }

    /// Count test ticks within a time range
    ///
    /// This is useful for verifying ticks from a specific test run.
    pub async fn count_test_ticks_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> DbVerifierResult<u64> {
        let row = sqlx::query(
            r#"
            SELECT COUNT(*) as count
            FROM market_ticks
            WHERE symbol LIKE 'TEST%'
              AND ts_event >= $1
              AND ts_event <= $2
            "#,
        )
        .bind(start)
        .bind(end)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.get::<i64, _>("count") as u64)
    }

    /// Count test ticks by exchange
    pub async fn count_test_ticks_by_exchange(
        &self,
        exchange: &str,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> DbVerifierResult<u64> {
        let query = match (start, end) {
            (Some(s), Some(e)) => sqlx::query(
                r#"
                    SELECT COUNT(*) as count
                    FROM market_ticks
                    WHERE symbol LIKE 'TEST%'
                      AND exchange = $1
                      AND ts_event >= $2
                      AND ts_event <= $3
                    "#,
            )
            .bind(exchange)
            .bind(s)
            .bind(e),
            _ => sqlx::query(
                r#"
                    SELECT COUNT(*) as count
                    FROM market_ticks
                    WHERE symbol LIKE 'TEST%'
                      AND exchange = $1
                    "#,
            )
            .bind(exchange),
        };

        let row = query.fetch_one(&self.pool).await?;
        Ok(row.get::<i64, _>("count") as u64)
    }

    /// Get per-symbol tick counts
    pub async fn get_symbol_counts(
        &self,
        exchange: &str,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> DbVerifierResult<HashMap<String, u64>> {
        let rows = match (start, end) {
            (Some(s), Some(e)) => {
                sqlx::query(
                    r#"
                    SELECT symbol, COUNT(*) as count
                    FROM market_ticks
                    WHERE symbol LIKE 'TEST%'
                      AND exchange = $1
                      AND ts_event >= $2
                      AND ts_event <= $3
                    GROUP BY symbol
                    ORDER BY symbol
                    "#,
                )
                .bind(exchange)
                .bind(s)
                .bind(e)
                .fetch_all(&self.pool)
                .await?
            }
            _ => {
                sqlx::query(
                    r#"
                    SELECT symbol, COUNT(*) as count
                    FROM market_ticks
                    WHERE symbol LIKE 'TEST%'
                      AND exchange = $1
                    GROUP BY symbol
                    ORDER BY symbol
                    "#,
                )
                .bind(exchange)
                .fetch_all(&self.pool)
                .await?
            }
        };

        let mut counts = HashMap::new();
        for row in rows {
            let symbol: String = row.get("symbol");
            let count: i64 = row.get("count");
            counts.insert(symbol, count as u64);
        }

        Ok(counts)
    }

    /// Get comprehensive statistics about persisted test ticks
    pub async fn get_test_tick_stats(
        &self,
        exchange: &str,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> DbVerifierResult<PersistedTickStats> {
        // Get overall stats
        let stats_row = match (start, end) {
            (Some(s), Some(e)) => {
                sqlx::query(
                    r#"
                    SELECT
                        COUNT(*) as total_count,
                        COUNT(DISTINCT symbol) as symbol_count,
                        MIN(ts_event) as earliest_time,
                        MAX(ts_event) as latest_time
                    FROM market_ticks
                    WHERE symbol LIKE 'TEST%'
                      AND exchange = $1
                      AND ts_event >= $2
                      AND ts_event <= $3
                    "#,
                )
                .bind(exchange)
                .bind(s)
                .bind(e)
                .fetch_one(&self.pool)
                .await?
            }
            _ => {
                sqlx::query(
                    r#"
                    SELECT
                        COUNT(*) as total_count,
                        COUNT(DISTINCT symbol) as symbol_count,
                        MIN(ts_event) as earliest_time,
                        MAX(ts_event) as latest_time
                    FROM market_ticks
                    WHERE symbol LIKE 'TEST%'
                      AND exchange = $1
                    "#,
                )
                .bind(exchange)
                .fetch_one(&self.pool)
                .await?
            }
        };

        // Get per-symbol counts
        let symbol_counts = self.get_symbol_counts(exchange, start, end).await?;

        Ok(PersistedTickStats {
            total_count: stats_row.get::<i64, _>("total_count") as u64,
            symbol_counts,
            earliest_time: stats_row.get("earliest_time"),
            latest_time: stats_row.get("latest_time"),
            symbol_count: stats_row.get::<i64, _>("symbol_count") as u64,
        })
    }

    /// Delete all test ticks (for cleanup)
    ///
    /// This removes all ticks with symbols matching TEST%.
    pub async fn cleanup_test_ticks(&self) -> DbVerifierResult<u64> {
        info!("Cleaning up test ticks from database...");

        let result = sqlx::query(
            r#"
            DELETE FROM market_ticks
            WHERE symbol LIKE 'TEST%'
            "#,
        )
        .execute(&self.pool)
        .await?;

        let count = result.rows_affected();
        debug!("Deleted {} test ticks", count);

        Ok(count)
    }

    /// Delete test ticks within a time range
    pub async fn cleanup_test_ticks_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> DbVerifierResult<u64> {
        info!("Cleaning up test ticks from {} to {}...", start, end);

        let result = sqlx::query(
            r#"
            DELETE FROM market_ticks
            WHERE symbol LIKE 'TEST%'
              AND ts_event >= $1
              AND ts_event <= $2
            "#,
        )
        .bind(start)
        .bind(end)
        .execute(&self.pool)
        .await?;

        let count = result.rows_affected();
        debug!("Deleted {} test ticks", count);

        Ok(count)
    }

    /// Verify that the expected number of ticks were persisted
    ///
    /// Returns (actual_count, expected_count, difference)
    pub async fn verify_tick_count(
        &self,
        exchange: &str,
        expected_count: u64,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> DbVerifierResult<(u64, u64, i64)> {
        let actual_count = self
            .count_test_ticks_by_exchange(exchange, start, end)
            .await?;
        let difference = actual_count as i64 - expected_count as i64;

        Ok((actual_count, expected_count, difference))
    }

    /// Close the database connection pool
    pub async fn close(self) {
        self.pool.close().await;
    }
}

/// Options for database verification in tests
#[derive(Debug, Clone)]
pub struct DbVerificationOptions {
    /// Whether to verify database persistence
    pub enabled: bool,
    /// Database URL to connect to
    pub database_url: Option<String>,
    /// Whether to clean up test data after verification
    pub cleanup_after_test: bool,
    /// Exchange name to query (default: "TEST")
    pub exchange: String,
    /// Tolerance for tick count mismatch (0.0 = exact match, 0.01 = 1% tolerance)
    pub count_tolerance: f64,
}

impl Default for DbVerificationOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            database_url: None,
            cleanup_after_test: true,
            exchange: "TEST".to_string(),
            count_tolerance: 0.01, // 1% tolerance by default
        }
    }
}

impl DbVerificationOptions {
    /// Create new options with database verification enabled
    pub fn enabled(database_url: String) -> Self {
        Self {
            enabled: true,
            database_url: Some(database_url),
            ..Default::default()
        }
    }

    /// Set the cleanup flag
    pub fn with_cleanup(mut self, cleanup: bool) -> Self {
        self.cleanup_after_test = cleanup;
        self
    }

    /// Set the exchange name
    pub fn with_exchange(mut self, exchange: String) -> Self {
        self.exchange = exchange;
        self
    }

    /// Set the count tolerance
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.count_tolerance = tolerance;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_options() {
        let opts = DbVerificationOptions::default();
        assert!(!opts.enabled);
        assert!(opts.database_url.is_none());
        assert!(opts.cleanup_after_test);
        assert_eq!(opts.exchange, "TEST");
    }

    #[test]
    fn test_enabled_options() {
        let opts = DbVerificationOptions::enabled("postgres://localhost/test".to_string());
        assert!(opts.enabled);
        assert_eq!(
            opts.database_url,
            Some("postgres://localhost/test".to_string())
        );
    }

    #[test]
    fn test_options_builder() {
        let opts = DbVerificationOptions::enabled("postgres://localhost/test".to_string())
            .with_cleanup(false)
            .with_exchange("BINANCE".to_string())
            .with_tolerance(0.05);

        assert!(opts.enabled);
        assert!(!opts.cleanup_after_test);
        assert_eq!(opts.exchange, "BINANCE");
        assert!((opts.count_tolerance - 0.05).abs() < 0.001);
    }
}
