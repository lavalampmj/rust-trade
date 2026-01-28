//! Exchange error types.

use thiserror::Error;
use trading_common::error::{ErrorCategory, ErrorClassification};

/// Error types for exchange operations
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum ExchangeError {
    #[allow(dead_code)]
    #[error("Network error: {0}")]
    Network(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Invalid symbol: {0}")]
    InvalidSymbol(String),

    #[error("Parse error: {0}")]
    Parse(String),
}

impl ErrorClassification for ExchangeError {
    fn category(&self) -> ErrorCategory {
        match self {
            ExchangeError::Network(_) => ErrorCategory::Transient,
            ExchangeError::Connection(_) => ErrorCategory::Transient,
            ExchangeError::InvalidSymbol(_) => ErrorCategory::Permanent,
            ExchangeError::Parse(_) => ErrorCategory::Permanent,
        }
    }

    fn suggested_retry_delay(&self) -> Option<std::time::Duration> {
        match self {
            ExchangeError::Network(_) => Some(std::time::Duration::from_secs(1)),
            ExchangeError::Connection(_) => Some(std::time::Duration::from_secs(2)),
            _ => None,
        }
    }
}

// Convert from common error types
impl From<serde_json::Error> for ExchangeError {
    fn from(err: serde_json::Error) -> Self {
        ExchangeError::Parse(err.to_string())
    }
}
