// exchange/errors.rs

use thiserror::Error;

/// Error types for exchange operations
#[derive(Error, Debug)]
pub enum ExchangeError {
    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Invalid symbol: {0}")]
    InvalidSymbol(String),

    #[error("Data parsing error: {0}")]
    ParseError(String),
}

// Convert from common error types
impl From<serde_json::Error> for ExchangeError {
    fn from(err: serde_json::Error) -> Self {
        ExchangeError::ParseError(err.to_string())
    }
}
