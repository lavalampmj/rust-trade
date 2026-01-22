//! Symbol repository for database operations on symbol definitions.
//!
//! This module provides CRUD operations for symbol definitions stored in the database,
//! including the symbol_definitions and market_calendars tables.
//!
//! # Example
//!
//! ```ignore
//! use trading_common::data::SymbolRepository;
//! use sqlx::PgPool;
//!
//! let repo = SymbolRepository::new(pool);
//!
//! // Get a symbol definition
//! let btcusdt = repo.get(&InstrumentId::new("BTCUSDT", "BINANCE")).await?;
//!
//! // List all symbols for a venue
//! let binance_symbols = repo.list_by_venue("BINANCE").await?;
//! ```

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use sqlx::{PgPool, Row};
use tracing::{debug, error, info, warn};

use crate::instruments::{
    ContractSpec, MarketCalendar, ProviderSymbol, SessionSchedule, SymbolDefinition, SymbolInfo,
    SymbolStatus, TradingSpecs, VenueConfig,
};
use crate::orders::InstrumentId;

use super::types::{DataError, DataResult};

/// Repository for symbol definition database operations.
pub struct SymbolRepository {
    pool: PgPool,
}

impl SymbolRepository {
    /// Create a new symbol repository
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get database pool reference
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    // =================================================================
    // Symbol Definition Operations
    // =================================================================

    /// Get a symbol definition by ID
    pub async fn get(&self, id: &InstrumentId) -> DataResult<Option<SymbolDefinition>> {
        let id_str = id.to_string();

        let row = sqlx::query(
            r#"
            SELECT
                id, symbol, venue, instrument_id, raw_symbol,
                info, venue_config, trading_specs,
                session_schedule, contract_spec,
                provider_mappings, status,
                created_at, updated_at
            FROM symbol_definitions
            WHERE id = $1
            "#,
        )
        .bind(&id_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            error!("Failed to fetch symbol definition {}: {}", id_str, e);
            DataError::Database(e)
        })?;

        match row {
            Some(row) => {
                let def = self.row_to_symbol_definition(&row)?;
                Ok(Some(def))
            }
            None => Ok(None),
        }
    }

    /// Get a symbol definition, returning error if not found
    pub async fn get_or_error(&self, id: &InstrumentId) -> DataResult<SymbolDefinition> {
        self.get(id).await?.ok_or_else(|| {
            DataError::NotFound(format!("Symbol definition not found: {}", id))
        })
    }

    /// Check if a symbol definition exists
    pub async fn exists(&self, id: &InstrumentId) -> DataResult<bool> {
        let id_str = id.to_string();

        let result: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM symbol_definitions WHERE id = $1",
        )
        .bind(&id_str)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DataError::Database(e))?;

        Ok(result.0 > 0)
    }

    /// Insert a new symbol definition
    pub async fn insert(&self, def: &SymbolDefinition) -> DataResult<()> {
        let id_str = def.id.to_string();

        debug!("Inserting symbol definition: {}", id_str);

        let info_json = serde_json::to_value(&def.info)
            .map_err(|e| DataError::Validation(format!("Failed to serialize info: {}", e)))?;

        let venue_config_json = serde_json::to_value(&def.venue_config)
            .map_err(|e| DataError::Validation(format!("Failed to serialize venue_config: {}", e)))?;

        let trading_specs_json = serde_json::to_value(&def.trading_specs)
            .map_err(|e| DataError::Validation(format!("Failed to serialize trading_specs: {}", e)))?;

        let session_schedule_json = def.session_schedule.as_ref()
            .map(|s| serde_json::to_value(s))
            .transpose()
            .map_err(|e| DataError::Validation(format!("Failed to serialize session_schedule: {}", e)))?;

        let contract_spec_json = def.contract_spec.as_ref()
            .map(|s| serde_json::to_value(s))
            .transpose()
            .map_err(|e| DataError::Validation(format!("Failed to serialize contract_spec: {}", e)))?;

        let provider_mappings_json = serde_json::to_value(&def.provider_mappings)
            .map_err(|e| DataError::Validation(format!("Failed to serialize provider_mappings: {}", e)))?;

        sqlx::query(
            r#"
            INSERT INTO symbol_definitions (
                id, symbol, venue, instrument_id, raw_symbol,
                info, venue_config, trading_specs,
                session_schedule, contract_spec,
                provider_mappings, status,
                created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, $8,
                $9, $10,
                $11, $12,
                $13, $14
            )
            "#,
        )
        .bind(&id_str)
        .bind(&def.id.symbol)
        .bind(&def.id.venue)
        .bind(def.instrument_id as i64)
        .bind(&def.raw_symbol)
        .bind(&info_json)
        .bind(&venue_config_json)
        .bind(&trading_specs_json)
        .bind(&session_schedule_json)
        .bind(&contract_spec_json)
        .bind(&provider_mappings_json)
        .bind(def.status.to_string())
        .bind(def.created_at)
        .bind(def.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            error!("Failed to insert symbol definition {}: {}", id_str, e);
            DataError::Database(e)
        })?;

        info!("Inserted symbol definition: {}", id_str);
        Ok(())
    }

    /// Update an existing symbol definition
    pub async fn update(&self, def: &SymbolDefinition) -> DataResult<()> {
        let id_str = def.id.to_string();

        debug!("Updating symbol definition: {}", id_str);

        let info_json = serde_json::to_value(&def.info)
            .map_err(|e| DataError::Validation(format!("Failed to serialize info: {}", e)))?;

        let venue_config_json = serde_json::to_value(&def.venue_config)
            .map_err(|e| DataError::Validation(format!("Failed to serialize venue_config: {}", e)))?;

        let trading_specs_json = serde_json::to_value(&def.trading_specs)
            .map_err(|e| DataError::Validation(format!("Failed to serialize trading_specs: {}", e)))?;

        let session_schedule_json = def.session_schedule.as_ref()
            .map(|s| serde_json::to_value(s))
            .transpose()
            .map_err(|e| DataError::Validation(format!("Failed to serialize session_schedule: {}", e)))?;

        let contract_spec_json = def.contract_spec.as_ref()
            .map(|s| serde_json::to_value(s))
            .transpose()
            .map_err(|e| DataError::Validation(format!("Failed to serialize contract_spec: {}", e)))?;

        let provider_mappings_json = serde_json::to_value(&def.provider_mappings)
            .map_err(|e| DataError::Validation(format!("Failed to serialize provider_mappings: {}", e)))?;

        let result = sqlx::query(
            r#"
            UPDATE symbol_definitions SET
                instrument_id = $2,
                raw_symbol = $3,
                info = $4,
                venue_config = $5,
                trading_specs = $6,
                session_schedule = $7,
                contract_spec = $8,
                provider_mappings = $9,
                status = $10,
                updated_at = $11
            WHERE id = $1
            "#,
        )
        .bind(&id_str)
        .bind(def.instrument_id as i64)
        .bind(&def.raw_symbol)
        .bind(&info_json)
        .bind(&venue_config_json)
        .bind(&trading_specs_json)
        .bind(&session_schedule_json)
        .bind(&contract_spec_json)
        .bind(&provider_mappings_json)
        .bind(def.status.to_string())
        .bind(Utc::now())
        .execute(&self.pool)
        .await
        .map_err(|e| {
            error!("Failed to update symbol definition {}: {}", id_str, e);
            DataError::Database(e)
        })?;

        if result.rows_affected() == 0 {
            return Err(DataError::NotFound(format!(
                "Symbol definition not found: {}",
                id_str
            )));
        }

        info!("Updated symbol definition: {}", id_str);
        Ok(())
    }

    /// Upsert (insert or update) a symbol definition
    pub async fn upsert(&self, def: &SymbolDefinition) -> DataResult<()> {
        let id_str = def.id.to_string();

        debug!("Upserting symbol definition: {}", id_str);

        let info_json = serde_json::to_value(&def.info)
            .map_err(|e| DataError::Validation(format!("Failed to serialize info: {}", e)))?;

        let venue_config_json = serde_json::to_value(&def.venue_config)
            .map_err(|e| DataError::Validation(format!("Failed to serialize venue_config: {}", e)))?;

        let trading_specs_json = serde_json::to_value(&def.trading_specs)
            .map_err(|e| DataError::Validation(format!("Failed to serialize trading_specs: {}", e)))?;

        let session_schedule_json = def.session_schedule.as_ref()
            .map(|s| serde_json::to_value(s))
            .transpose()
            .map_err(|e| DataError::Validation(format!("Failed to serialize session_schedule: {}", e)))?;

        let contract_spec_json = def.contract_spec.as_ref()
            .map(|s| serde_json::to_value(s))
            .transpose()
            .map_err(|e| DataError::Validation(format!("Failed to serialize contract_spec: {}", e)))?;

        let provider_mappings_json = serde_json::to_value(&def.provider_mappings)
            .map_err(|e| DataError::Validation(format!("Failed to serialize provider_mappings: {}", e)))?;

        sqlx::query(
            r#"
            INSERT INTO symbol_definitions (
                id, symbol, venue, instrument_id, raw_symbol,
                info, venue_config, trading_specs,
                session_schedule, contract_spec,
                provider_mappings, status,
                created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, $8,
                $9, $10,
                $11, $12,
                $13, $14
            )
            ON CONFLICT (id) DO UPDATE SET
                instrument_id = EXCLUDED.instrument_id,
                raw_symbol = EXCLUDED.raw_symbol,
                info = EXCLUDED.info,
                venue_config = EXCLUDED.venue_config,
                trading_specs = EXCLUDED.trading_specs,
                session_schedule = EXCLUDED.session_schedule,
                contract_spec = EXCLUDED.contract_spec,
                provider_mappings = EXCLUDED.provider_mappings,
                status = EXCLUDED.status,
                updated_at = NOW()
            "#,
        )
        .bind(&id_str)
        .bind(&def.id.symbol)
        .bind(&def.id.venue)
        .bind(def.instrument_id as i64)
        .bind(&def.raw_symbol)
        .bind(&info_json)
        .bind(&venue_config_json)
        .bind(&trading_specs_json)
        .bind(&session_schedule_json)
        .bind(&contract_spec_json)
        .bind(&provider_mappings_json)
        .bind(def.status.to_string())
        .bind(def.created_at)
        .bind(def.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            error!("Failed to upsert symbol definition {}: {}", id_str, e);
            DataError::Database(e)
        })?;

        info!("Upserted symbol definition: {}", id_str);
        Ok(())
    }

    /// Delete a symbol definition
    pub async fn delete(&self, id: &InstrumentId) -> DataResult<bool> {
        let id_str = id.to_string();

        debug!("Deleting symbol definition: {}", id_str);

        let result = sqlx::query("DELETE FROM symbol_definitions WHERE id = $1")
            .bind(&id_str)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                error!("Failed to delete symbol definition {}: {}", id_str, e);
                DataError::Database(e)
            })?;

        let deleted = result.rows_affected() > 0;
        if deleted {
            info!("Deleted symbol definition: {}", id_str);
        }
        Ok(deleted)
    }

    /// List all symbol definitions
    pub async fn list_all(&self) -> DataResult<Vec<SymbolDefinition>> {
        let rows = sqlx::query(
            r#"
            SELECT
                id, symbol, venue, instrument_id, raw_symbol,
                info, venue_config, trading_specs,
                session_schedule, contract_spec,
                provider_mappings, status,
                created_at, updated_at
            FROM symbol_definitions
            ORDER BY venue, symbol
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            error!("Failed to list all symbol definitions: {}", e);
            DataError::Database(e)
        })?;

        let mut definitions = Vec::with_capacity(rows.len());
        for row in rows {
            definitions.push(self.row_to_symbol_definition(&row)?);
        }

        Ok(definitions)
    }

    /// List symbol definitions by venue
    pub async fn list_by_venue(&self, venue: &str) -> DataResult<Vec<SymbolDefinition>> {
        let rows = sqlx::query(
            r#"
            SELECT
                id, symbol, venue, instrument_id, raw_symbol,
                info, venue_config, trading_specs,
                session_schedule, contract_spec,
                provider_mappings, status,
                created_at, updated_at
            FROM symbol_definitions
            WHERE venue = $1
            ORDER BY symbol
            "#,
        )
        .bind(venue)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            error!("Failed to list symbols for venue {}: {}", venue, e);
            DataError::Database(e)
        })?;

        let mut definitions = Vec::with_capacity(rows.len());
        for row in rows {
            definitions.push(self.row_to_symbol_definition(&row)?);
        }

        Ok(definitions)
    }

    /// List symbol definitions by status
    pub async fn list_by_status(&self, status: SymbolStatus) -> DataResult<Vec<SymbolDefinition>> {
        let rows = sqlx::query(
            r#"
            SELECT
                id, symbol, venue, instrument_id, raw_symbol,
                info, venue_config, trading_specs,
                session_schedule, contract_spec,
                provider_mappings, status,
                created_at, updated_at
            FROM symbol_definitions
            WHERE status = $1
            ORDER BY venue, symbol
            "#,
        )
        .bind(status.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            error!("Failed to list symbols by status {}: {}", status, e);
            DataError::Database(e)
        })?;

        let mut definitions = Vec::with_capacity(rows.len());
        for row in rows {
            definitions.push(self.row_to_symbol_definition(&row)?);
        }

        Ok(definitions)
    }

    /// Search symbols by pattern
    pub async fn search(&self, pattern: &str) -> DataResult<Vec<SymbolDefinition>> {
        let search_pattern = format!("%{}%", pattern.to_uppercase());

        let rows = sqlx::query(
            r#"
            SELECT
                id, symbol, venue, instrument_id, raw_symbol,
                info, venue_config, trading_specs,
                session_schedule, contract_spec,
                provider_mappings, status,
                created_at, updated_at
            FROM symbol_definitions
            WHERE UPPER(symbol) LIKE $1 OR UPPER(raw_symbol) LIKE $1
            ORDER BY venue, symbol
            LIMIT 100
            "#,
        )
        .bind(&search_pattern)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            error!("Failed to search symbols with pattern {}: {}", pattern, e);
            DataError::Database(e)
        })?;

        let mut definitions = Vec::with_capacity(rows.len());
        for row in rows {
            definitions.push(self.row_to_symbol_definition(&row)?);
        }

        Ok(definitions)
    }

    /// Count total symbol definitions
    pub async fn count(&self) -> DataResult<i64> {
        let result: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM symbol_definitions")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DataError::Database(e))?;

        Ok(result.0)
    }

    /// Count symbol definitions by venue
    pub async fn count_by_venue(&self, venue: &str) -> DataResult<i64> {
        let result: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM symbol_definitions WHERE venue = $1",
        )
        .bind(venue)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DataError::Database(e))?;

        Ok(result.0)
    }

    // =================================================================
    // Market Calendar Operations
    // =================================================================

    /// Get market calendar for a venue within a date range
    pub async fn get_calendar(
        &self,
        venue: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> DataResult<MarketCalendar> {
        let rows = sqlx::query(
            r#"
            SELECT date, calendar_type, time, description
            FROM market_calendars
            WHERE venue = $1 AND date >= $2 AND date <= $3
            ORDER BY date
            "#,
        )
        .bind(venue)
        .bind(start_date)
        .bind(end_date)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            error!("Failed to fetch market calendar for {}: {}", venue, e);
            DataError::Database(e)
        })?;

        let mut calendar = MarketCalendar::new();

        for row in rows {
            let date: NaiveDate = row.get("date");
            let calendar_type: String = row.get("calendar_type");
            let time: Option<NaiveTime> = row.get("time");
            let description: Option<String> = row.get("description");

            match calendar_type.as_str() {
                "holiday" => {
                    calendar.add_holiday(date, description);
                }
                "early_close" => {
                    if let Some(t) = time {
                        calendar.add_early_close(date, t);
                    }
                }
                "late_open" => {
                    if let Some(t) = time {
                        calendar.add_late_open(date, t);
                    }
                }
                _ => {
                    warn!("Unknown calendar type: {}", calendar_type);
                }
            }
        }

        Ok(calendar)
    }

    /// Add a holiday to the market calendar
    pub async fn add_holiday(
        &self,
        venue: &str,
        date: NaiveDate,
        description: Option<&str>,
    ) -> DataResult<()> {
        sqlx::query(
            r#"
            INSERT INTO market_calendars (venue, date, calendar_type, description)
            VALUES ($1, $2, 'holiday', $3)
            ON CONFLICT (venue, date, calendar_type) DO UPDATE SET
                description = EXCLUDED.description
            "#,
        )
        .bind(venue)
        .bind(date)
        .bind(description)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            error!("Failed to add holiday for {} on {}: {}", venue, date, e);
            DataError::Database(e)
        })?;

        Ok(())
    }

    /// Add an early close to the market calendar
    pub async fn add_early_close(
        &self,
        venue: &str,
        date: NaiveDate,
        close_time: NaiveTime,
        description: Option<&str>,
    ) -> DataResult<()> {
        sqlx::query(
            r#"
            INSERT INTO market_calendars (venue, date, calendar_type, time, description)
            VALUES ($1, $2, 'early_close', $3, $4)
            ON CONFLICT (venue, date, calendar_type) DO UPDATE SET
                time = EXCLUDED.time,
                description = EXCLUDED.description
            "#,
        )
        .bind(venue)
        .bind(date)
        .bind(close_time)
        .bind(description)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            error!("Failed to add early close for {} on {}: {}", venue, date, e);
            DataError::Database(e)
        })?;

        Ok(())
    }

    /// Add a late open to the market calendar
    pub async fn add_late_open(
        &self,
        venue: &str,
        date: NaiveDate,
        open_time: NaiveTime,
        description: Option<&str>,
    ) -> DataResult<()> {
        sqlx::query(
            r#"
            INSERT INTO market_calendars (venue, date, calendar_type, time, description)
            VALUES ($1, $2, 'late_open', $3, $4)
            ON CONFLICT (venue, date, calendar_type) DO UPDATE SET
                time = EXCLUDED.time,
                description = EXCLUDED.description
            "#,
        )
        .bind(venue)
        .bind(date)
        .bind(open_time)
        .bind(description)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            error!("Failed to add late open for {} on {}: {}", venue, date, e);
            DataError::Database(e)
        })?;

        Ok(())
    }

    /// Check if a specific date is a holiday
    pub async fn is_holiday(&self, venue: &str, date: NaiveDate) -> DataResult<bool> {
        let result: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*) FROM market_calendars
            WHERE venue = $1 AND date = $2 AND calendar_type = 'holiday'
            "#,
        )
        .bind(venue)
        .bind(date)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DataError::Database(e))?;

        Ok(result.0 > 0)
    }

    // =================================================================
    // Helper Methods
    // =================================================================

    /// Convert a database row to SymbolDefinition
    fn row_to_symbol_definition(&self, row: &sqlx::postgres::PgRow) -> DataResult<SymbolDefinition> {
        let symbol: String = row.get("symbol");
        let venue: String = row.get("venue");
        let instrument_id: i64 = row.get("instrument_id");
        let raw_symbol: String = row.get("raw_symbol");

        let info: serde_json::Value = row.get("info");
        let info: SymbolInfo = serde_json::from_value(info)
            .map_err(|e| DataError::Validation(format!("Failed to deserialize info: {}", e)))?;

        let venue_config: serde_json::Value = row.get("venue_config");
        let venue_config: VenueConfig = serde_json::from_value(venue_config)
            .map_err(|e| DataError::Validation(format!("Failed to deserialize venue_config: {}", e)))?;

        let trading_specs: serde_json::Value = row.get("trading_specs");
        let trading_specs: TradingSpecs = serde_json::from_value(trading_specs)
            .map_err(|e| DataError::Validation(format!("Failed to deserialize trading_specs: {}", e)))?;

        let session_schedule: Option<serde_json::Value> = row.get("session_schedule");
        let session_schedule: Option<SessionSchedule> = session_schedule
            .map(|v| serde_json::from_value(v))
            .transpose()
            .map_err(|e| DataError::Validation(format!("Failed to deserialize session_schedule: {}", e)))?;

        let contract_spec: Option<serde_json::Value> = row.get("contract_spec");
        let contract_spec: Option<ContractSpec> = contract_spec
            .map(|v| serde_json::from_value(v))
            .transpose()
            .map_err(|e| DataError::Validation(format!("Failed to deserialize contract_spec: {}", e)))?;

        let provider_mappings: serde_json::Value = row.get("provider_mappings");
        let provider_mappings: std::collections::HashMap<String, ProviderSymbol> =
            serde_json::from_value(provider_mappings).map_err(|e| {
                DataError::Validation(format!("Failed to deserialize provider_mappings: {}", e))
            })?;

        let status_str: String = row.get("status");
        let status = match status_str.as_str() {
            "ACTIVE" => SymbolStatus::Active,
            "HALTED" => SymbolStatus::Halted,
            "SUSPENDED" => SymbolStatus::Suspended,
            "DELISTED" => SymbolStatus::Delisted,
            "PENDING" => SymbolStatus::Pending,
            "EXPIRED" => SymbolStatus::Expired,
            _ => SymbolStatus::Active,
        };

        let created_at: DateTime<Utc> = row.get("created_at");
        let updated_at: DateTime<Utc> = row.get("updated_at");

        Ok(SymbolDefinition {
            id: InstrumentId::new(&symbol, &venue),
            instrument_id: instrument_id as u64,
            raw_symbol,
            info,
            venue_config,
            trading_specs,
            session_schedule,
            contract_spec,
            provider_mappings,
            status,
            ts_recv: Utc::now(),
            created_at,
            updated_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Integration tests would require a test database
    // These are placeholder tests for the type conversions

    #[test]
    fn test_symbol_status_conversion() {
        assert_eq!(SymbolStatus::Active.to_string(), "ACTIVE");
        assert_eq!(SymbolStatus::Halted.to_string(), "HALTED");
        assert_eq!(SymbolStatus::Suspended.to_string(), "SUSPENDED");
        assert_eq!(SymbolStatus::Delisted.to_string(), "DELISTED");
        assert_eq!(SymbolStatus::Pending.to_string(), "PENDING");
        assert_eq!(SymbolStatus::Expired.to_string(), "EXPIRED");
    }
}
