//! Common error types shared across crates.
//!
//! These error types represent common failure modes that occur throughout
//! the trading system. Crate-specific errors can wrap these using `#[from]`.

use std::time::Duration;
use thiserror::Error;

/// Database-related errors.
///
/// Use this for all database operations including queries, connections,
/// and transactions.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum DatabaseError {
    /// Query execution failed
    #[error("Query failed: {0}")]
    Query(String),

    /// Connection to database failed
    #[error("Database connection failed: {0}")]
    Connection(String),

    /// Transaction failed (commit, rollback, etc.)
    #[error("Transaction failed: {0}")]
    Transaction(String),

    /// Connection pool exhausted
    #[error("Connection pool exhausted")]
    PoolExhausted,

    /// Query timeout
    #[error("Query timeout after {0:?}")]
    Timeout(Duration),
}

impl From<sqlx::Error> for DatabaseError {
    fn from(err: sqlx::Error) -> Self {
        match &err {
            sqlx::Error::PoolTimedOut => DatabaseError::PoolExhausted,
            sqlx::Error::Io(_) => DatabaseError::Connection(err.to_string()),
            _ => DatabaseError::Query(err.to_string()),
        }
    }
}

/// Network-related errors.
///
/// Use this for network operations including WebSocket connections,
/// HTTP requests, and IPC communication.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum NetworkError {
    /// Connection failed
    #[error("Connection failed: {0}")]
    Connection(String),

    /// Connection timeout
    #[error("Connection timeout after {0:?}")]
    Timeout(Duration),

    /// Request failed
    #[error("Request failed: {0}")]
    Request(String),

    /// Send operation failed
    #[error("Send failed: {0}")]
    Send(String),

    /// Receive operation failed
    #[error("Receive failed: {0}")]
    Receive(String),

    /// Connection was closed
    #[error("Connection closed: {0}")]
    Closed(String),

    /// DNS resolution failed
    #[error("DNS resolution failed: {0}")]
    DnsError(String),
}

/// Configuration-related errors.
///
/// Use this for configuration loading, parsing, and validation.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum ConfigurationError {
    /// Required field is missing
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// Field has invalid value
    #[error("Invalid value for '{field}': {reason}")]
    InvalidValue { field: String, reason: String },

    /// Configuration file could not be parsed
    #[error("Parse error: {0}")]
    Parse(String),

    /// Configuration file not found
    #[error("Configuration file not found: {0}")]
    FileNotFound(String),

    /// Environment variable not set
    #[error("Environment variable not set: {0}")]
    EnvVarMissing(String),

    /// Invalid configuration combination
    #[error("Invalid configuration: {0}")]
    Invalid(String),
}

/// Entity-related errors for CRUD operations.
///
/// Use this for operations on domain entities like orders, symbols, etc.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum EntityError {
    /// Entity was not found
    #[error("{entity_type} not found: {id}")]
    NotFound {
        entity_type: &'static str,
        id: String,
    },

    /// Entity already exists (duplicate)
    #[error("{entity_type} already exists: {id}")]
    AlreadyExists {
        entity_type: &'static str,
        id: String,
    },

    /// Entity validation failed
    #[error("Invalid {entity_type}: {reason}")]
    Invalid {
        entity_type: &'static str,
        reason: String,
    },

    /// Entity is in wrong state for operation
    #[error("{entity_type} '{id}' is in invalid state: {reason}")]
    InvalidState {
        entity_type: &'static str,
        id: String,
        reason: String,
    },
}

impl EntityError {
    /// Create a NotFound error
    pub fn not_found(entity_type: &'static str, id: impl Into<String>) -> Self {
        EntityError::NotFound {
            entity_type,
            id: id.into(),
        }
    }

    /// Create an AlreadyExists error
    pub fn already_exists(entity_type: &'static str, id: impl Into<String>) -> Self {
        EntityError::AlreadyExists {
            entity_type,
            id: id.into(),
        }
    }

    /// Create an Invalid error
    pub fn invalid(entity_type: &'static str, reason: impl Into<String>) -> Self {
        EntityError::Invalid {
            entity_type,
            reason: reason.into(),
        }
    }
}

/// Serialization and parsing errors.
///
/// Use this for JSON, binary, or other format conversions.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum SerializationError {
    /// JSON serialization/deserialization failed
    #[error("JSON error: {0}")]
    Json(String),

    /// Binary serialization failed
    #[error("Binary serialization error: {0}")]
    Binary(String),

    /// Invalid format
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    /// Decimal conversion failed
    #[error("Decimal conversion error: {0}")]
    Decimal(String),
}

impl From<serde_json::Error> for SerializationError {
    fn from(err: serde_json::Error) -> Self {
        SerializationError::Json(err.to_string())
    }
}

impl From<rust_decimal::Error> for SerializationError {
    fn from(err: rust_decimal::Error) -> Self {
        SerializationError::Decimal(err.to_string())
    }
}

/// Cache-related errors.
///
/// Use this for Redis, in-memory cache, or other caching operations.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum CacheError {
    /// Cache connection failed
    #[error("Cache connection failed: {0}")]
    Connection(String),

    /// Cache read failed
    #[error("Cache read failed: {0}")]
    Read(String),

    /// Cache write failed
    #[error("Cache write failed: {0}")]
    Write(String),

    /// Cache key not found
    #[error("Cache key not found: {0}")]
    NotFound(String),

    /// Cache serialization failed
    #[error("Cache serialization error: {0}")]
    Serialization(String),

    /// Cache is unavailable (degraded mode)
    #[error("Cache unavailable")]
    Unavailable,
}

/// Channel/queue communication errors.
///
/// Use this for mpsc, broadcast, and other async channel operations.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum ChannelError {
    /// Channel is closed
    #[error("Channel closed")]
    Closed,

    /// Send failed (receiver dropped)
    #[error("Send failed: {0}")]
    SendFailed(String),

    /// Receive failed (sender dropped)
    #[error("Receive failed: {0}")]
    ReceiveFailed(String),

    /// Channel is full (backpressure)
    #[error("Channel full (capacity: {capacity})")]
    Full { capacity: usize },

    /// Channel timeout
    #[error("Channel operation timeout")]
    Timeout,
}

/// Validation errors for data integrity checks.
///
/// Use this for validating incoming data before processing.
#[derive(Error, Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ValidationError {
    /// Value is out of allowed range
    #[error("{field} value {value} is out of range [{min}, {max}]")]
    OutOfRange {
        field: &'static str,
        value: String,
        min: String,
        max: String,
    },

    /// Required field is empty or missing
    #[error("{field} is required but was empty")]
    Required { field: &'static str },

    /// Field exceeds maximum length
    #[error("{field} exceeds maximum length of {max_length}")]
    TooLong {
        field: &'static str,
        max_length: usize,
    },

    /// Field contains invalid characters
    #[error("{field} contains invalid characters: {reason}")]
    InvalidCharacters { field: &'static str, reason: String },

    /// Field has invalid format
    #[error("{field} has invalid format: {reason}")]
    InvalidFormat { field: &'static str, reason: String },

    /// Custom validation failed
    #[error("Validation failed: {0}")]
    Custom(String),
}

impl ValidationError {
    /// Create a Required validation error
    pub fn required(field: &'static str) -> Self {
        ValidationError::Required { field }
    }

    /// Create an OutOfRange validation error
    pub fn out_of_range(
        field: &'static str,
        value: impl ToString,
        min: impl ToString,
        max: impl ToString,
    ) -> Self {
        ValidationError::OutOfRange {
            field,
            value: value.to_string(),
            min: min.to_string(),
            max: max.to_string(),
        }
    }

    /// Create a TooLong validation error
    pub fn too_long(field: &'static str, max_length: usize) -> Self {
        ValidationError::TooLong { field, max_length }
    }

    /// Create an InvalidCharacters validation error
    pub fn invalid_chars(field: &'static str, reason: impl Into<String>) -> Self {
        ValidationError::InvalidCharacters {
            field,
            reason: reason.into(),
        }
    }

    /// Create an InvalidFormat validation error
    pub fn invalid_format(field: &'static str, reason: impl Into<String>) -> Self {
        ValidationError::InvalidFormat {
            field,
            reason: reason.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_error_constructors() {
        let err = EntityError::not_found("Order", "ORD-123");
        assert!(err.to_string().contains("Order not found: ORD-123"));

        let err = EntityError::already_exists("Symbol", "BTCUSDT");
        assert!(err.to_string().contains("Symbol already exists: BTCUSDT"));

        let err = EntityError::invalid("Price", "must be positive");
        assert!(err.to_string().contains("Invalid Price: must be positive"));
    }

    #[test]
    fn test_validation_error_constructors() {
        let err = ValidationError::required("symbol");
        assert!(err.to_string().contains("symbol is required"));

        let err = ValidationError::out_of_range("price", "1000000", "0", "100000");
        assert!(err.to_string().contains("out of range"));

        let err = ValidationError::too_long("trade_id", 64);
        assert!(err.to_string().contains("exceeds maximum length"));
    }

    #[test]
    fn test_database_error_from_sqlx() {
        // We can't easily create sqlx errors, but we can verify the enum variants exist
        let err = DatabaseError::Query("test query error".to_string());
        assert!(err.to_string().contains("Query failed"));

        let err = DatabaseError::Connection("connection refused".to_string());
        assert!(err.to_string().contains("connection failed"));
    }

    #[test]
    fn test_configuration_error() {
        let err = ConfigurationError::MissingField("database_url".to_string());
        assert!(err.to_string().contains("Missing required field"));

        let err = ConfigurationError::InvalidValue {
            field: "port".to_string(),
            reason: "must be between 1 and 65535".to_string(),
        };
        assert!(err.to_string().contains("Invalid value for 'port'"));
    }
}
