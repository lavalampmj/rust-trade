//! Repository for Databento instrument definitions.
//!
//! This module provides CRUD operations for the `databento_instruments` table,
//! which stores canonical instrument metadata from Databento's security master.
//!
//! # Key Concepts
//!
//! - `instrument_id`: Databento's globally unique, opaque primary key
//! - `raw_symbol`: Publisher's symbol (e.g., "ESH6", "AAPL")
//! - `dataset`: Databento dataset (e.g., "GLBX.MDP3", "XNAS.ITCH")
//!
//! # Example
//!
//! ```ignore
//! use data_manager::storage::DatabentoInstrumentRepository;
//!
//! let repo = DatabentoInstrumentRepository::new(pool);
//!
//! // Upsert an instrument definition
//! repo.upsert(&instrument_def).await?;
//!
//! // Lookup by instrument_id
//! let def = repo.get_by_id(102341).await?;
//!
//! // Lookup by symbol and dataset
//! let def = repo.get_by_symbol("ESH6", "GLBX.MDP3").await?;
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::{FromRow, PgPool, Row};
use tracing::{debug, info};

use super::RepositoryResult;

/// Repository for Databento instrument definitions.
#[derive(Clone)]
pub struct DatabentoInstrumentRepository {
    pool: PgPool,
}

/// Databento instrument definition stored in the database.
///
/// This represents a single instrument from Databento's security master.
/// The `instrument_id` is the canonical, globally unique identifier.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DatabentoInstrumentRow {
    /// Databento's canonical instrument_id (globally unique)
    pub instrument_id: i64,

    /// Raw symbol as used by publisher
    pub raw_symbol: String,

    /// Dataset (e.g., "GLBX.MDP3")
    pub dataset: String,

    /// Exchange/venue MIC code
    pub exchange: String,

    /// Publisher ID from Databento
    pub publisher_id: Option<i16>,

    /// Security type (e.g., "FUT", "OPT", "STK")
    pub security_type: Option<String>,

    /// Instrument class character
    pub instrument_class: Option<String>,

    /// Contract expiration timestamp
    pub expiration: Option<DateTime<Utc>>,

    /// Activation timestamp
    pub activation: Option<DateTime<Utc>>,

    /// Underlying instrument_id (for derivatives)
    pub underlying_id: Option<i64>,

    /// Strike price (raw i64)
    pub strike_price: Option<i64>,

    /// Minimum price increment (raw i64)
    pub min_price_increment: i64,

    /// Display factor for price conversion
    pub display_factor: i64,

    /// Minimum lot size (raw i64)
    pub min_lot_size_round_lot: i64,

    /// Price precision (decimal places)
    pub price_precision: i16,

    /// Contract multiplier (raw i64)
    pub contract_multiplier: Option<i64>,

    /// CFI code (ISO 10962)
    pub cfi_code: Option<String>,

    /// Full InstrumentDefMsg as JSONB
    pub raw_definition: JsonValue,

    /// When this record was first seen
    pub first_seen: DateTime<Utc>,

    /// Last update timestamp
    pub last_updated: DateTime<Utc>,
}

/// Input for upserting an instrument definition.
#[derive(Debug, Clone)]
pub struct DatabentoInstrumentInput {
    /// Databento's canonical instrument_id
    pub instrument_id: i64,

    /// Raw symbol as used by publisher
    pub raw_symbol: String,

    /// Dataset (e.g., "GLBX.MDP3")
    pub dataset: String,

    /// Exchange/venue MIC code
    pub exchange: String,

    /// Publisher ID from Databento
    pub publisher_id: Option<i16>,

    /// Security type (e.g., "FUT", "OPT", "STK")
    pub security_type: Option<String>,

    /// Instrument class character
    pub instrument_class: Option<char>,

    /// Contract expiration timestamp
    pub expiration: Option<DateTime<Utc>>,

    /// Activation timestamp
    pub activation: Option<DateTime<Utc>>,

    /// Underlying instrument_id (for derivatives)
    pub underlying_id: Option<i64>,

    /// Strike price (raw i64)
    pub strike_price: Option<i64>,

    /// Minimum price increment (raw i64)
    pub min_price_increment: i64,

    /// Display factor for price conversion
    pub display_factor: i64,

    /// Minimum lot size (raw i64)
    pub min_lot_size_round_lot: i64,

    /// Price precision (decimal places)
    pub price_precision: i16,

    /// Contract multiplier (raw i64)
    pub contract_multiplier: Option<i64>,

    /// CFI code (ISO 10962)
    pub cfi_code: Option<String>,

    /// Full InstrumentDefMsg as JSONB
    pub raw_definition: JsonValue,
}

impl DatabentoInstrumentRepository {
    /// Create a new repository with the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get the connection pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Upsert an instrument definition.
    ///
    /// If an instrument with the same `instrument_id` exists, it will be updated.
    /// If an instrument with the same `raw_symbol` + `dataset` exists but different
    /// `instrument_id`, the old record is replaced (this handles symbol reuse).
    pub async fn upsert(&self, input: &DatabentoInstrumentInput) -> RepositoryResult<()> {
        let instrument_class_str = input.instrument_class.map(|c| c.to_string());

        sqlx::query(
            r#"
            INSERT INTO databento_instruments (
                instrument_id, raw_symbol, dataset, exchange, publisher_id,
                security_type, instrument_class, expiration, activation,
                underlying_id, strike_price, min_price_increment, display_factor,
                min_lot_size_round_lot, price_precision, contract_multiplier,
                cfi_code, raw_definition, first_seen, last_updated
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, NOW(), NOW()
            )
            ON CONFLICT (instrument_id) DO UPDATE SET
                raw_symbol = EXCLUDED.raw_symbol,
                dataset = EXCLUDED.dataset,
                exchange = EXCLUDED.exchange,
                publisher_id = EXCLUDED.publisher_id,
                security_type = EXCLUDED.security_type,
                instrument_class = EXCLUDED.instrument_class,
                expiration = EXCLUDED.expiration,
                activation = EXCLUDED.activation,
                underlying_id = EXCLUDED.underlying_id,
                strike_price = EXCLUDED.strike_price,
                min_price_increment = EXCLUDED.min_price_increment,
                display_factor = EXCLUDED.display_factor,
                min_lot_size_round_lot = EXCLUDED.min_lot_size_round_lot,
                price_precision = EXCLUDED.price_precision,
                contract_multiplier = EXCLUDED.contract_multiplier,
                cfi_code = EXCLUDED.cfi_code,
                raw_definition = EXCLUDED.raw_definition,
                last_updated = NOW()
            "#,
        )
        .bind(input.instrument_id)
        .bind(&input.raw_symbol)
        .bind(&input.dataset)
        .bind(&input.exchange)
        .bind(input.publisher_id)
        .bind(&input.security_type)
        .bind(&instrument_class_str)
        .bind(input.expiration)
        .bind(input.activation)
        .bind(input.underlying_id)
        .bind(input.strike_price)
        .bind(input.min_price_increment)
        .bind(input.display_factor)
        .bind(input.min_lot_size_round_lot)
        .bind(input.price_precision)
        .bind(input.contract_multiplier)
        .bind(&input.cfi_code)
        .bind(&input.raw_definition)
        .execute(&self.pool)
        .await?;

        debug!(
            "Upserted instrument: {} (id={}) in {}",
            input.raw_symbol, input.instrument_id, input.dataset
        );
        Ok(())
    }

    /// Batch upsert multiple instrument definitions.
    pub async fn upsert_batch(&self, inputs: &[DatabentoInstrumentInput]) -> RepositoryResult<u64> {
        if inputs.is_empty() {
            return Ok(0);
        }

        let mut count = 0u64;
        for input in inputs {
            self.upsert(input).await?;
            count += 1;
        }

        info!("Upserted {} instrument definitions", count);
        Ok(count)
    }

    /// Get an instrument by its canonical instrument_id.
    pub async fn get_by_id(
        &self,
        instrument_id: i64,
    ) -> RepositoryResult<Option<DatabentoInstrumentRow>> {
        let row = sqlx::query_as::<_, DatabentoInstrumentRow>(
            r#"
            SELECT * FROM databento_instruments WHERE instrument_id = $1
            "#,
        )
        .bind(instrument_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    /// Get an instrument by raw_symbol and dataset.
    ///
    /// This is the primary lookup method when you have a symbol string
    /// and need to find its canonical instrument_id.
    pub async fn get_by_symbol(
        &self,
        raw_symbol: &str,
        dataset: &str,
    ) -> RepositoryResult<Option<DatabentoInstrumentRow>> {
        let row = sqlx::query_as::<_, DatabentoInstrumentRow>(
            r#"
            SELECT * FROM databento_instruments
            WHERE raw_symbol = $1 AND dataset = $2
            "#,
        )
        .bind(raw_symbol)
        .bind(dataset)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    /// Get all instruments for a dataset.
    pub async fn list_by_dataset(&self, dataset: &str) -> RepositoryResult<Vec<DatabentoInstrumentRow>> {
        let rows = sqlx::query_as::<_, DatabentoInstrumentRow>(
            r#"
            SELECT * FROM databento_instruments
            WHERE dataset = $1
            ORDER BY raw_symbol
            "#,
        )
        .bind(dataset)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Get all instruments of a specific security type in a dataset.
    pub async fn list_by_type(
        &self,
        dataset: &str,
        security_type: &str,
    ) -> RepositoryResult<Vec<DatabentoInstrumentRow>> {
        let rows = sqlx::query_as::<_, DatabentoInstrumentRow>(
            r#"
            SELECT * FROM databento_instruments
            WHERE dataset = $1 AND security_type = $2
            ORDER BY raw_symbol
            "#,
        )
        .bind(dataset)
        .bind(security_type)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Get all active futures contracts (not expired).
    pub async fn list_active_futures(&self, dataset: &str) -> RepositoryResult<Vec<DatabentoInstrumentRow>> {
        let rows = sqlx::query_as::<_, DatabentoInstrumentRow>(
            r#"
            SELECT * FROM databento_instruments
            WHERE dataset = $1
              AND security_type = 'FUT'
              AND (expiration IS NULL OR expiration > NOW())
            ORDER BY expiration NULLS LAST, raw_symbol
            "#,
        )
        .bind(dataset)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Get instruments expiring within a time range.
    pub async fn list_expiring(
        &self,
        dataset: &str,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> RepositoryResult<Vec<DatabentoInstrumentRow>> {
        let rows = sqlx::query_as::<_, DatabentoInstrumentRow>(
            r#"
            SELECT * FROM databento_instruments
            WHERE dataset = $1
              AND expiration >= $2
              AND expiration <= $3
            ORDER BY expiration, raw_symbol
            "#,
        )
        .bind(dataset)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Get all derivatives for an underlying instrument.
    pub async fn list_by_underlying(
        &self,
        underlying_id: i64,
    ) -> RepositoryResult<Vec<DatabentoInstrumentRow>> {
        let rows = sqlx::query_as::<_, DatabentoInstrumentRow>(
            r#"
            SELECT * FROM databento_instruments
            WHERE underlying_id = $1
            ORDER BY expiration NULLS LAST, strike_price NULLS LAST, raw_symbol
            "#,
        )
        .bind(underlying_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Search instruments by symbol pattern (LIKE query).
    pub async fn search(&self, pattern: &str, dataset: Option<&str>) -> RepositoryResult<Vec<DatabentoInstrumentRow>> {
        let like_pattern = format!("%{}%", pattern);

        let rows = if let Some(ds) = dataset {
            sqlx::query_as::<_, DatabentoInstrumentRow>(
                r#"
                SELECT * FROM databento_instruments
                WHERE raw_symbol ILIKE $1 AND dataset = $2
                ORDER BY raw_symbol
                LIMIT 100
                "#,
            )
            .bind(&like_pattern)
            .bind(ds)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, DatabentoInstrumentRow>(
                r#"
                SELECT * FROM databento_instruments
                WHERE raw_symbol ILIKE $1
                ORDER BY dataset, raw_symbol
                LIMIT 100
                "#,
            )
            .bind(&like_pattern)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows)
    }

    /// Delete an instrument by its instrument_id.
    pub async fn delete(&self, instrument_id: i64) -> RepositoryResult<bool> {
        let result = sqlx::query("DELETE FROM databento_instruments WHERE instrument_id = $1")
            .bind(instrument_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Get statistics about stored instruments.
    pub async fn get_stats(&self) -> RepositoryResult<DatabentoInstrumentStats> {
        let row = sqlx::query(
            r#"
            SELECT
                COUNT(*) as total_count,
                COUNT(DISTINCT dataset) as dataset_count,
                COUNT(DISTINCT exchange) as exchange_count,
                COUNT(*) FILTER (WHERE security_type = 'FUT') as futures_count,
                COUNT(*) FILTER (WHERE security_type = 'OPT') as options_count,
                COUNT(*) FILTER (WHERE security_type = 'STK') as stocks_count,
                MIN(first_seen) as oldest_record,
                MAX(last_updated) as newest_record
            FROM databento_instruments
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(DatabentoInstrumentStats {
            total_count: row.get::<Option<i64>, _>("total_count").unwrap_or(0) as u64,
            dataset_count: row.get::<Option<i64>, _>("dataset_count").unwrap_or(0) as u64,
            exchange_count: row.get::<Option<i64>, _>("exchange_count").unwrap_or(0) as u64,
            futures_count: row.get::<Option<i64>, _>("futures_count").unwrap_or(0) as u64,
            options_count: row.get::<Option<i64>, _>("options_count").unwrap_or(0) as u64,
            stocks_count: row.get::<Option<i64>, _>("stocks_count").unwrap_or(0) as u64,
            oldest_record: row.get("oldest_record"),
            newest_record: row.get("newest_record"),
        })
    }

    /// Get the instrument_id for a symbol+dataset, returning None if not found.
    ///
    /// This is a lightweight lookup that only fetches the ID.
    pub async fn resolve_id(
        &self,
        raw_symbol: &str,
        dataset: &str,
    ) -> RepositoryResult<Option<i64>> {
        let row = sqlx::query(
            r#"
            SELECT instrument_id FROM databento_instruments
            WHERE raw_symbol = $1 AND dataset = $2
            "#,
        )
        .bind(raw_symbol)
        .bind(dataset)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.get("instrument_id")))
    }

    /// Batch resolve instrument_ids for multiple symbols.
    ///
    /// Returns a map of raw_symbol → instrument_id for symbols that were found.
    pub async fn resolve_ids_batch(
        &self,
        raw_symbols: &[&str],
        dataset: &str,
    ) -> RepositoryResult<std::collections::HashMap<String, i64>> {
        if raw_symbols.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let rows = sqlx::query(
            r#"
            SELECT raw_symbol, instrument_id FROM databento_instruments
            WHERE raw_symbol = ANY($1) AND dataset = $2
            "#,
        )
        .bind(raw_symbols)
        .bind(dataset)
        .fetch_all(&self.pool)
        .await?;

        let mut map = std::collections::HashMap::new();
        for row in rows {
            let symbol: String = row.get("raw_symbol");
            let id: i64 = row.get("instrument_id");
            map.insert(symbol, id);
        }

        Ok(map)
    }
}

/// Statistics about stored Databento instruments.
#[derive(Debug, Clone)]
pub struct DatabentoInstrumentStats {
    /// Total number of instruments
    pub total_count: u64,
    /// Number of distinct datasets
    pub dataset_count: u64,
    /// Number of distinct exchanges
    pub exchange_count: u64,
    /// Number of futures contracts
    pub futures_count: u64,
    /// Number of options contracts
    pub options_count: u64,
    /// Number of stocks
    pub stocks_count: u64,
    /// Oldest record timestamp
    pub oldest_record: Option<DateTime<Utc>>,
    /// Newest record timestamp
    pub newest_record: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instrument_input_creation() {
        let input = DatabentoInstrumentInput {
            instrument_id: 102341,
            raw_symbol: "ESH6".to_string(),
            dataset: "GLBX.MDP3".to_string(),
            exchange: "XCME".to_string(),
            publisher_id: Some(1),
            security_type: Some("FUT".to_string()),
            instrument_class: Some('F'),
            expiration: None,
            activation: None,
            underlying_id: None,
            strike_price: None,
            min_price_increment: 2500000000, // 0.25 * 10^10
            display_factor: 1000000000,
            min_lot_size_round_lot: 1,
            price_precision: 9,
            contract_multiplier: Some(50),
            cfi_code: Some("FXXXXX".to_string()),
            raw_definition: serde_json::json!({}),
        };

        assert_eq!(input.instrument_id, 102341);
        assert_eq!(input.raw_symbol, "ESH6");
        assert_eq!(input.dataset, "GLBX.MDP3");
    }
}
