//! Database-backed symbol registry
//!
//! Provides persistent storage for symbol metadata and provider mappings.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use tracing::debug;

use crate::storage::RepositoryError;
use super::{SymbolSpec, SymbolStatus};

/// Symbol registry backed by the database
pub struct SymbolRegistry {
    pool: PgPool,
}

impl SymbolRegistry {
    /// Create a new symbol registry
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Register a new symbol
    pub async fn register(&self, spec: &SymbolSpec) -> Result<i32, RepositoryError> {
        let provider_symbols = serde_json::to_value(&spec.provider_mapping)
            .map_err(|e| RepositoryError::InvalidData(e.to_string()))?;

        let row = sqlx::query(
            r#"
            INSERT INTO symbol_registry (symbol, exchange, provider_symbols, status)
            VALUES ($1, $2, $3, 'active')
            ON CONFLICT (symbol, exchange) DO UPDATE
            SET provider_symbols = $3, updated_at = NOW()
            RETURNING id
            "#,
        )
        .bind(&spec.symbol)
        .bind(&spec.exchange)
        .bind(provider_symbols)
        .fetch_one(&self.pool)
        .await?;

        let id: i32 = row.get("id");
        debug!("Registered symbol {} with id {}", spec, id);
        Ok(id)
    }

    /// Register multiple symbols
    pub async fn register_many(&self, specs: &[SymbolSpec]) -> Result<usize, RepositoryError> {
        let mut count = 0;
        for spec in specs {
            self.register(spec).await?;
            count += 1;
        }
        Ok(count)
    }

    /// Get a symbol by its specification
    pub async fn get(&self, symbol: &str, exchange: &str) -> Result<Option<RegisteredSymbol>, RepositoryError> {
        let row = sqlx::query(
            r#"
            SELECT id, symbol, exchange, provider_symbols, status,
                   data_start, data_end, created_at, updated_at
            FROM symbol_registry
            WHERE symbol = $1 AND exchange = $2
            "#,
        )
        .bind(symbol)
        .bind(exchange)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(row_to_registered_symbol(&row)?)),
            None => Ok(None),
        }
    }

    /// List all registered symbols
    pub async fn list(&self) -> Result<Vec<RegisteredSymbol>, RepositoryError> {
        let rows = sqlx::query(
            r#"
            SELECT id, symbol, exchange, provider_symbols, status,
                   data_start, data_end, created_at, updated_at
            FROM symbol_registry
            ORDER BY exchange, symbol
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let symbols: Result<Vec<_>, _> = rows.iter().map(row_to_registered_symbol).collect();
        symbols
    }

    /// List symbols by exchange
    pub async fn list_by_exchange(&self, exchange: &str) -> Result<Vec<RegisteredSymbol>, RepositoryError> {
        let rows = sqlx::query(
            r#"
            SELECT id, symbol, exchange, provider_symbols, status,
                   data_start, data_end, created_at, updated_at
            FROM symbol_registry
            WHERE exchange = $1
            ORDER BY symbol
            "#,
        )
        .bind(exchange)
        .fetch_all(&self.pool)
        .await?;

        let symbols: Result<Vec<_>, _> = rows.iter().map(row_to_registered_symbol).collect();
        symbols
    }

    /// List symbols by status
    pub async fn list_by_status(&self, status: SymbolStatus) -> Result<Vec<RegisteredSymbol>, RepositoryError> {
        let rows = sqlx::query(
            r#"
            SELECT id, symbol, exchange, provider_symbols, status,
                   data_start, data_end, created_at, updated_at
            FROM symbol_registry
            WHERE status = $1
            ORDER BY exchange, symbol
            "#,
        )
        .bind(status.as_str())
        .fetch_all(&self.pool)
        .await?;

        let symbols: Result<Vec<_>, _> = rows.iter().map(row_to_registered_symbol).collect();
        symbols
    }

    /// Update symbol status
    pub async fn update_status(
        &self,
        symbol: &str,
        exchange: &str,
        status: SymbolStatus,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            r#"
            UPDATE symbol_registry
            SET status = $3, updated_at = NOW()
            WHERE symbol = $1 AND exchange = $2
            "#,
        )
        .bind(symbol)
        .bind(exchange)
        .bind(status.as_str())
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Update data availability range
    pub async fn update_data_range(
        &self,
        symbol: &str,
        exchange: &str,
        data_start: Option<DateTime<Utc>>,
        data_end: Option<DateTime<Utc>>,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            r#"
            UPDATE symbol_registry
            SET data_start = COALESCE($3, data_start),
                data_end = COALESCE($4, data_end),
                updated_at = NOW()
            WHERE symbol = $1 AND exchange = $2
            "#,
        )
        .bind(symbol)
        .bind(exchange)
        .bind(data_start)
        .bind(data_end)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Delete a symbol from the registry
    pub async fn delete(&self, symbol: &str, exchange: &str) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            r#"
            DELETE FROM symbol_registry
            WHERE symbol = $1 AND exchange = $2
            "#,
        )
        .bind(symbol)
        .bind(exchange)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Get count of registered symbols
    pub async fn count(&self) -> Result<i64, RepositoryError> {
        let row = sqlx::query("SELECT COUNT(*) as count FROM symbol_registry")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get("count"))
    }
}

/// A symbol registered in the database
#[derive(Debug, Clone)]
pub struct RegisteredSymbol {
    pub id: i32,
    pub spec: SymbolSpec,
    pub status: SymbolStatus,
    pub data_start: Option<DateTime<Utc>>,
    pub data_end: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn row_to_registered_symbol(row: &sqlx::postgres::PgRow) -> Result<RegisteredSymbol, RepositoryError> {
    let symbol: String = row.get("symbol");
    let exchange: String = row.get("exchange");
    let provider_symbols: serde_json::Value = row.get("provider_symbols");
    let status_str: String = row.get("status");

    let provider_mapping: HashMap<String, String> = serde_json::from_value(provider_symbols)
        .unwrap_or_default();

    let mut spec = SymbolSpec::new(symbol, exchange);
    spec.provider_mapping = provider_mapping;

    let status = SymbolStatus::from_str(&status_str).unwrap_or(SymbolStatus::Pending);

    Ok(RegisteredSymbol {
        id: row.get("id"),
        spec,
        status,
        data_start: row.get("data_start"),
        data_end: row.get("data_end"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_status() {
        assert_eq!(SymbolStatus::Active.as_str(), "active");
        assert_eq!(SymbolStatus::from_str("active"), Some(SymbolStatus::Active));
        assert_eq!(SymbolStatus::from_str("unknown"), None);
    }
}
