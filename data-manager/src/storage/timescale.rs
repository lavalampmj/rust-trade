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

        // Create databento_instruments table for storing canonical instrument definitions
        // from Databento's security master. The instrument_id is globally unique and stable.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS databento_instruments (
                -- Databento's canonical instrument_id (globally unique, never reused)
                instrument_id BIGINT PRIMARY KEY,

                -- Raw symbol as used by publisher (e.g., "ESH6", "AAPL")
                raw_symbol VARCHAR(50) NOT NULL,

                -- Dataset (e.g., "GLBX.MDP3", "XNAS.ITCH")
                dataset VARCHAR(50) NOT NULL,

                -- Exchange/venue MIC code (e.g., "XCME", "XNAS")
                exchange VARCHAR(20) NOT NULL,

                -- Publisher ID from Databento
                publisher_id SMALLINT,

                -- Security type from Databento (e.g., "FUT", "OPT", "STK")
                security_type VARCHAR(10),

                -- Instrument class character (F=Future, O=Option, K=Stock, etc.)
                instrument_class CHAR(1),

                -- Contract expiration timestamp (for futures/options)
                expiration TIMESTAMPTZ,

                -- Activation timestamp (when instrument becomes tradeable)
                activation TIMESTAMPTZ,

                -- Underlying instrument_id (for derivatives)
                underlying_id BIGINT,

                -- Strike price (for options, stored as raw i64)
                strike_price BIGINT,

                -- Pricing parameters (stored as raw i64 for exact DBN reproduction)
                min_price_increment BIGINT NOT NULL DEFAULT 0,
                display_factor BIGINT NOT NULL DEFAULT 1000000000,
                min_lot_size_round_lot BIGINT NOT NULL DEFAULT 1,

                -- Price precision (number of decimal places)
                price_precision SMALLINT NOT NULL DEFAULT 9,

                -- Contract multiplier (for futures, stored as raw)
                contract_multiplier BIGINT,

                -- CFI code (ISO 10962)
                cfi_code VARCHAR(10),

                -- Full InstrumentDefMsg as JSONB for complete data access
                raw_definition JSONB NOT NULL,

                -- Timestamps
                first_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                last_updated TIMESTAMPTZ NOT NULL DEFAULT NOW(),

                -- Composite unique for symbol+dataset lookups
                CONSTRAINT uq_databento_symbol_dataset UNIQUE (raw_symbol, dataset)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create indexes for databento_instruments
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_dbt_instruments_raw_symbol
            ON databento_instruments (raw_symbol)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_dbt_instruments_dataset
            ON databento_instruments (dataset)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_dbt_instruments_exchange
            ON databento_instruments (exchange)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_dbt_instruments_expiration
            ON databento_instruments (expiration)
            WHERE expiration IS NOT NULL
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_dbt_instruments_security_type
            ON databento_instruments (security_type)
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create index on underlying_id for derivative lookups
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_dbt_instruments_underlying
            ON databento_instruments (underlying_id)
            WHERE underlying_id IS NOT NULL
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Add new columns to databento_instruments (safe to run multiple times)
        // These columns were added to capture more of the InstrumentDefMsg fields
        let new_columns = [
            ("ts_recv", "BIGINT"),
            ("currency", "VARCHAR(4)"),
            ("settl_currency", "VARCHAR(4)"),
            ("asset", "VARCHAR(11)"),
            ("security_group", "VARCHAR(21)"),
            ("unit_of_measure", "VARCHAR(31)"),
            ("underlying", "VARCHAR(21)"),
            ("maturity_year", "SMALLINT"),
            ("maturity_month", "SMALLINT"),
            ("maturity_day", "SMALLINT"),
            ("high_limit_price", "BIGINT"),
            ("low_limit_price", "BIGINT"),
            ("channel_id", "SMALLINT"),
        ];

        for (column, col_type) in &new_columns {
            let query = format!(
                "ALTER TABLE databento_instruments ADD COLUMN IF NOT EXISTS {} {}",
                column, col_type
            );
            if let Err(e) = sqlx::query(&query).execute(&self.pool).await {
                // Ignore errors for already existing columns
                debug!("Column {} may already exist: {}", column, e);
            }
        }

        // Create indexes for the new columns
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_dbt_instruments_asset
            ON databento_instruments (asset)
            WHERE asset IS NOT NULL
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_dbt_instruments_currency
            ON databento_instruments (currency)
            WHERE currency IS NOT NULL
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_dbt_instruments_maturity
            ON databento_instruments (maturity_year, maturity_month)
            WHERE maturity_year IS NOT NULL
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_dbt_instruments_asset_expiry
            ON databento_instruments (dataset, asset, expiration)
            WHERE asset IS NOT NULL
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
            uncompressed_chunks: row
                .get::<Option<i64>, _>("uncompressed_chunks")
                .unwrap_or(0) as u64,
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
            _ => {
                return Err(RepositoryError::InvalidData(format!(
                    "Unsupported timeframe: {}",
                    timeframe
                )))
            }
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

    /// Add a refresh policy to a continuous aggregate
    ///
    /// This schedules automatic refreshes of the aggregate:
    /// - `start_offset`: How far back from real-time to start refreshing (e.g., "1 hour")
    /// - `end_offset`: How far back from real-time to stop refreshing (e.g., "1 minute")
    /// - `schedule_interval`: How often to run the refresh (e.g., "1 minute")
    pub async fn add_aggregate_refresh_policy(
        &self,
        view_name: &str,
        start_offset: &str,
        end_offset: &str,
        schedule_interval: &str,
    ) -> RepositoryResult<()> {
        info!(
            "Adding refresh policy to {}: start_offset={}, end_offset={}, schedule={}",
            view_name, start_offset, end_offset, schedule_interval
        );

        let query = format!(
            r#"
            SELECT add_continuous_aggregate_policy(
                '{}',
                start_offset => INTERVAL '{}',
                end_offset => INTERVAL '{}',
                schedule_interval => INTERVAL '{}',
                if_not_exists => TRUE
            )
            "#,
            view_name, start_offset, end_offset, schedule_interval
        );

        sqlx::query(&query).execute(&self.pool).await?;
        info!("Added refresh policy for {}", view_name);
        Ok(())
    }

    /// Remove a refresh policy from a continuous aggregate
    pub async fn remove_aggregate_refresh_policy(&self, view_name: &str) -> RepositoryResult<()> {
        info!("Removing refresh policy from {}...", view_name);

        let query = format!(
            r#"SELECT remove_continuous_aggregate_policy('{}', if_exists => TRUE)"#,
            view_name
        );

        sqlx::query(&query).execute(&self.pool).await?;
        info!("Removed refresh policy from {}", view_name);
        Ok(())
    }

    /// Get information about continuous aggregates
    pub async fn get_aggregate_info(&self) -> RepositoryResult<Vec<AggregateInfo>> {
        let rows = sqlx::query(
            r#"
            SELECT
                cav.view_name::TEXT as view_name,
                cav.view_owner::TEXT as view_owner,
                cap.refresh_interval::TEXT as refresh_interval,
                cap.refresh_start_offset::TEXT as start_offset,
                cap.refresh_end_offset::TEXT as end_offset,
                (SELECT COUNT(*) FROM timescaledb_information.chunks
                 WHERE hypertable_name = cav.materialization_hypertable_name)::BIGINT as chunk_count
            FROM timescaledb_information.continuous_aggregates cav
            LEFT JOIN timescaledb_information.jobs j
                ON j.hypertable_name = cav.materialization_hypertable_name
            LEFT JOIN timescaledb_information.job_stats js ON js.job_id = j.job_id
            LEFT JOIN LATERAL (
                SELECT
                    j2.schedule_interval as refresh_interval,
                    j2.config->>'start_offset' as refresh_start_offset,
                    j2.config->>'end_offset' as refresh_end_offset
                FROM timescaledb_information.jobs j2
                WHERE j2.proc_name = 'policy_refresh_continuous_aggregate'
                  AND j2.hypertable_name = cav.materialization_hypertable_name
                LIMIT 1
            ) cap ON TRUE
            WHERE cav.view_schema = 'public'
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut aggregates = Vec::new();
        for row in rows {
            aggregates.push(AggregateInfo {
                view_name: row.get("view_name"),
                view_owner: row.get("view_owner"),
                refresh_interval: row.get("refresh_interval"),
                start_offset: row.get("start_offset"),
                end_offset: row.get("end_offset"),
                chunk_count: row.get::<Option<i64>, _>("chunk_count").unwrap_or(0) as u64,
            });
        }

        Ok(aggregates)
    }

    /// Drop a continuous aggregate
    pub async fn drop_aggregate(&self, view_name: &str) -> RepositoryResult<()> {
        info!("Dropping continuous aggregate {}...", view_name);

        let query = format!("DROP MATERIALIZED VIEW IF EXISTS {} CASCADE", view_name);
        sqlx::query(&query).execute(&self.pool).await?;

        info!("Dropped continuous aggregate {}", view_name);
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

/// Continuous aggregate information
#[derive(Debug, Clone)]
pub struct AggregateInfo {
    pub view_name: String,
    pub view_owner: String,
    pub refresh_interval: Option<String>,
    pub start_offset: Option<String>,
    pub end_offset: Option<String>,
    pub chunk_count: u64,
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

-- =============================================================================
-- Databento Instrument Definitions
-- Stores canonical instrument metadata from Databento's security master.
-- The instrument_id is globally unique and stable across time.
-- =============================================================================

CREATE TABLE IF NOT EXISTS databento_instruments (
    -- Databento's canonical instrument_id (globally unique, never reused)
    instrument_id BIGINT PRIMARY KEY,

    -- Raw symbol as used by publisher (e.g., "ESH6", "AAPL")
    raw_symbol VARCHAR(50) NOT NULL,

    -- Dataset (e.g., "GLBX.MDP3", "XNAS.ITCH")
    dataset VARCHAR(50) NOT NULL,

    -- Exchange/venue MIC code (e.g., "XCME", "XNAS")
    exchange VARCHAR(20) NOT NULL,

    -- Publisher ID from Databento
    publisher_id SMALLINT,

    -- Security type from Databento (e.g., "FUT", "OPT", "STK")
    security_type VARCHAR(10),

    -- Instrument class character (F=Future, O=Option, K=Stock, etc.)
    instrument_class CHAR(1),

    -- Contract expiration timestamp (for futures/options)
    expiration TIMESTAMPTZ,

    -- Activation timestamp (when instrument becomes tradeable)
    activation TIMESTAMPTZ,

    -- Underlying instrument_id (for derivatives)
    underlying_id BIGINT,

    -- Strike price (for options, stored as raw i64)
    strike_price BIGINT,

    -- Pricing parameters (stored as raw i64 for exact DBN reproduction)
    min_price_increment BIGINT NOT NULL DEFAULT 0,
    display_factor BIGINT NOT NULL DEFAULT 1000000000,
    min_lot_size_round_lot BIGINT NOT NULL DEFAULT 1,

    -- Price precision (number of decimal places)
    price_precision SMALLINT NOT NULL DEFAULT 9,

    -- Contract multiplier (for futures, stored as raw)
    contract_multiplier BIGINT,

    -- CFI code (ISO 10962)
    cfi_code VARCHAR(10),

    -- Full InstrumentDefMsg as JSONB for complete data access
    raw_definition JSONB NOT NULL,

    -- Timestamps
    first_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_updated TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Composite unique for symbol+dataset lookups
    CONSTRAINT uq_databento_symbol_dataset UNIQUE (raw_symbol, dataset)
);

-- Indexes for databento_instruments
CREATE INDEX IF NOT EXISTS idx_dbt_instruments_raw_symbol ON databento_instruments (raw_symbol);
CREATE INDEX IF NOT EXISTS idx_dbt_instruments_dataset ON databento_instruments (dataset);
CREATE INDEX IF NOT EXISTS idx_dbt_instruments_exchange ON databento_instruments (exchange);
CREATE INDEX IF NOT EXISTS idx_dbt_instruments_expiration ON databento_instruments (expiration) WHERE expiration IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_dbt_instruments_security_type ON databento_instruments (security_type);
CREATE INDEX IF NOT EXISTS idx_dbt_instruments_underlying ON databento_instruments (underlying_id) WHERE underlying_id IS NOT NULL;
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
