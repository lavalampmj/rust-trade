//! TimescaleDB-specific operations
//!
//! Provides helpers for TimescaleDB features like hypertables,
//! compression, and continuous aggregates.

use sqlx::{PgPool, Row};
use tracing::{debug, info, warn};

use super::{RepositoryError, RepositoryResult};

/// TimescaleDB operations
pub struct TimescaleOperations {
    pool: PgPool,
}

impl TimescaleOperations {
    /// Create a new TimescaleDB operations helper
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Run database migrations
    pub async fn run_migrations(&self) -> RepositoryResult<()> {
        info!("Running TimescaleDB migrations...");

        // Create TimescaleDB extension if not exists
        sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb CASCADE")
            .execute(&self.pool)
            .await?;

        // Create market_ticks table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS market_ticks (
                ts_event TIMESTAMPTZ NOT NULL,
                ts_recv TIMESTAMPTZ NOT NULL,
                symbol VARCHAR(32) NOT NULL,
                exchange VARCHAR(16) NOT NULL,
                price NUMERIC(20, 8) NOT NULL,
                size NUMERIC(20, 8) NOT NULL,
                side CHAR(1) NOT NULL,
                provider VARCHAR(16) NOT NULL,
                provider_trade_id VARCHAR(64),
                is_buyer_maker BOOLEAN,
                raw_dbn BYTEA,
                sequence BIGINT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Convert to hypertable (will fail gracefully if already a hypertable)
        let result = sqlx::query(
            r#"
            SELECT create_hypertable(
                'market_ticks',
                'ts_event',
                chunk_time_interval => INTERVAL '1 day',
                if_not_exists => TRUE
            )
            "#,
        )
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => info!("Created market_ticks hypertable"),
            Err(e) => {
                if e.to_string().contains("already a hypertable") {
                    debug!("market_ticks is already a hypertable");
                } else {
                    warn!("Failed to create hypertable: {}", e);
                }
            }
        }

        // Create indexes
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_market_ticks_symbol_ts
            ON market_ticks (symbol, exchange, ts_event DESC)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_market_ticks_provider
            ON market_ticks (provider, ts_event DESC)
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create symbol_registry table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS symbol_registry (
                id SERIAL PRIMARY KEY,
                symbol VARCHAR(32) NOT NULL,
                exchange VARCHAR(16) NOT NULL,
                provider_symbols JSONB NOT NULL DEFAULT '{}',
                status VARCHAR(16) NOT NULL DEFAULT 'active',
                data_start TIMESTAMPTZ,
                data_end TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE (symbol, exchange)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create data_jobs table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS data_jobs (
                id UUID PRIMARY KEY,
                job_type VARCHAR(32) NOT NULL,
                symbols JSONB NOT NULL,
                status VARCHAR(16) NOT NULL DEFAULT 'pending',
                progress_pct SMALLINT DEFAULT 0,
                records_processed BIGINT DEFAULT 0,
                error_message TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        info!("TimescaleDB migrations completed");
        Ok(())
    }

    /// Enable compression on market_ticks
    pub async fn enable_compression(&self) -> RepositoryResult<()> {
        info!("Enabling compression on market_ticks...");

        sqlx::query(
            r#"
            ALTER TABLE market_ticks SET (
                timescaledb.compress,
                timescaledb.compress_segmentby = 'symbol,exchange',
                timescaledb.compress_orderby = 'ts_event DESC, sequence DESC'
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        info!("Compression enabled");
        Ok(())
    }

    /// Add compression policy
    pub async fn add_compression_policy(&self, after_days: i32) -> RepositoryResult<()> {
        info!(
            "Adding compression policy (compress after {} days)...",
            after_days
        );

        let query = format!(
            r#"
            SELECT add_compression_policy(
                'market_ticks',
                INTERVAL '{} days',
                if_not_exists => TRUE
            )
            "#,
            after_days
        );

        sqlx::query(&query).execute(&self.pool).await?;

        info!("Compression policy added");
        Ok(())
    }

    /// Manually compress old chunks
    pub async fn compress_chunks_older_than(&self, days: i32) -> RepositoryResult<u64> {
        info!("Compressing chunks older than {} days...", days);

        let query = format!(
            r#"
            SELECT compress_chunk(c.chunk_schema || '.' || c.chunk_name)
            FROM timescaledb_information.chunks c
            WHERE c.hypertable_name = 'market_ticks'
              AND c.range_end < NOW() - INTERVAL '{} days'
              AND NOT c.is_compressed
            "#,
            days
        );

        let result = sqlx::query(&query).execute(&self.pool).await?;
        let count = result.rows_affected();

        info!("Compressed {} chunks", count);
        Ok(count)
    }

    /// Get compression statistics
    pub async fn get_compression_stats(&self) -> RepositoryResult<CompressionStats> {
        let row = sqlx::query(
            r#"
            SELECT
                SUM(CASE WHEN is_compressed THEN 1 ELSE 0 END)::BIGINT as compressed_chunks,
                SUM(CASE WHEN NOT is_compressed THEN 1 ELSE 0 END)::BIGINT as uncompressed_chunks,
                pg_size_pretty(SUM(CASE WHEN is_compressed THEN total_bytes ELSE 0 END)) as compressed_size,
                pg_size_pretty(SUM(CASE WHEN NOT is_compressed THEN total_bytes ELSE 0 END)) as uncompressed_size
            FROM timescaledb_information.chunks c
            LEFT JOIN LATERAL (
                SELECT pg_total_relation_size(format('%I.%I', c.chunk_schema, c.chunk_name)) as total_bytes
            ) s ON TRUE
            WHERE c.hypertable_name = 'market_ticks'
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(CompressionStats {
            compressed_chunks: row.get::<Option<i64>, _>("compressed_chunks").unwrap_or(0) as u64,
            uncompressed_chunks: row.get::<Option<i64>, _>("uncompressed_chunks").unwrap_or(0)
                as u64,
            compressed_size: row
                .get::<Option<String>, _>("compressed_size")
                .unwrap_or_else(|| "0 bytes".to_string()),
            uncompressed_size: row
                .get::<Option<String>, _>("uncompressed_size")
                .unwrap_or_else(|| "0 bytes".to_string()),
        })
    }

    /// Create a continuous aggregate for OHLC data
    pub async fn create_ohlc_aggregate(&self, timeframe: &str) -> RepositoryResult<()> {
        let interval = match timeframe {
            "1m" => "1 minute",
            "5m" => "5 minutes",
            "15m" => "15 minutes",
            "1h" => "1 hour",
            "4h" => "4 hours",
            "1d" => "1 day",
            _ => return Err(RepositoryError::InvalidData(format!(
                "Unsupported timeframe: {}",
                timeframe
            ))),
        };

        let view_name = format!("ohlc_{}", timeframe);

        info!("Creating continuous aggregate {}...", view_name);

        let query = format!(
            r#"
            CREATE MATERIALIZED VIEW IF NOT EXISTS {} WITH (timescaledb.continuous) AS
            SELECT
                time_bucket(INTERVAL '{}', ts_event) AS bucket,
                symbol,
                exchange,
                FIRST(price, ts_event) AS open,
                MAX(price) AS high,
                MIN(price) AS low,
                LAST(price, ts_event) AS close,
                SUM(size) AS volume,
                COUNT(*) AS trade_count
            FROM market_ticks
            GROUP BY bucket, symbol, exchange
            WITH NO DATA
            "#,
            view_name, interval
        );

        sqlx::query(&query).execute(&self.pool).await?;

        info!("Created continuous aggregate {}", view_name);
        Ok(())
    }

    /// Refresh a continuous aggregate
    pub async fn refresh_aggregate(
        &self,
        view_name: &str,
        start: &str,
        end: &str,
    ) -> RepositoryResult<()> {
        let query = format!(
            r#"
            CALL refresh_continuous_aggregate('{}', '{}', '{}')
            "#,
            view_name, start, end
        );

        sqlx::query(&query).execute(&self.pool).await?;
        Ok(())
    }
}

/// Compression statistics
#[derive(Debug, Clone)]
pub struct CompressionStats {
    pub compressed_chunks: u64,
    pub uncompressed_chunks: u64,
    pub compressed_size: String,
    pub uncompressed_size: String,
}

/// SQL migration script
pub const MIGRATION_SQL: &str = r#"
-- TimescaleDB Data Manager Schema
-- Run this to initialize the database

-- Enable TimescaleDB
CREATE EXTENSION IF NOT EXISTS timescaledb CASCADE;

-- Market ticks table (main time-series data)
CREATE TABLE IF NOT EXISTS market_ticks (
    ts_event TIMESTAMPTZ NOT NULL,
    ts_recv TIMESTAMPTZ NOT NULL,
    symbol VARCHAR(32) NOT NULL,
    exchange VARCHAR(16) NOT NULL,
    price NUMERIC(20, 8) NOT NULL,
    size NUMERIC(20, 8) NOT NULL,
    side CHAR(1) NOT NULL,
    provider VARCHAR(16) NOT NULL,
    provider_trade_id VARCHAR(64),
    is_buyer_maker BOOLEAN,
    raw_dbn BYTEA,
    sequence BIGINT NOT NULL
);

-- Convert to hypertable with 1-day chunks
SELECT create_hypertable(
    'market_ticks',
    'ts_event',
    chunk_time_interval => INTERVAL '1 day',
    if_not_exists => TRUE
);

-- Create indexes
CREATE INDEX IF NOT EXISTS idx_market_ticks_symbol_ts
ON market_ticks (symbol, exchange, ts_event DESC);

CREATE INDEX IF NOT EXISTS idx_market_ticks_provider
ON market_ticks (provider, ts_event DESC);

-- Enable compression
ALTER TABLE market_ticks SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'symbol,exchange',
    timescaledb.compress_orderby = 'ts_event DESC, sequence DESC'
);

-- Add compression policy (compress after 7 days)
SELECT add_compression_policy('market_ticks', INTERVAL '7 days', if_not_exists => TRUE);

-- Symbol registry table
CREATE TABLE IF NOT EXISTS symbol_registry (
    id SERIAL PRIMARY KEY,
    symbol VARCHAR(32) NOT NULL,
    exchange VARCHAR(16) NOT NULL,
    provider_symbols JSONB NOT NULL DEFAULT '{}',
    status VARCHAR(16) NOT NULL DEFAULT 'active',
    data_start TIMESTAMPTZ,
    data_end TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (symbol, exchange)
);

-- Data jobs table
CREATE TABLE IF NOT EXISTS data_jobs (
    id UUID PRIMARY KEY,
    job_type VARCHAR(32) NOT NULL,
    symbols JSONB NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'pending',
    progress_pct SMALLINT DEFAULT 0,
    records_processed BIGINT DEFAULT 0,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Create indexes for data_jobs
CREATE INDEX IF NOT EXISTS idx_data_jobs_status ON data_jobs (status);
CREATE INDEX IF NOT EXISTS idx_data_jobs_created ON data_jobs (created_at DESC);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration_sql_syntax() {
        // Just verify the SQL constant is valid
        assert!(MIGRATION_SQL.contains("CREATE TABLE"));
        assert!(MIGRATION_SQL.contains("create_hypertable"));
    }
}
