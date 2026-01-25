//! Service layer error types.

use crate::exchange::ExchangeError;
use thiserror::Error;
use trading_common::data::types::DataError;
use trading_common::error::{ErrorCategory, ErrorClassification};

/// Service layer error types
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum ServiceError {
    #[error("Exchange error: {0}")]
    Exchange(#[from] ExchangeError),

    #[error("Data error: {0}")]
    Data(#[from] DataError),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Task error: {0}")]
    Task(String),
}

impl ErrorClassification for ServiceError {
    fn category(&self) -> ErrorCategory {
        match self {
            ServiceError::Exchange(e) => e.category(),
            ServiceError::Data(e) => e.category(),
            ServiceError::Configuration(_) => ErrorCategory::Configuration,
            ServiceError::Task(_) => ErrorCategory::Internal,
        }
    }

    fn suggested_retry_delay(&self) -> Option<std::time::Duration> {
        match self {
            ServiceError::Exchange(e) => e.suggested_retry_delay(),
            ServiceError::Data(e) => e.suggested_retry_delay(),
            _ => None,
        }
    }
}

/// Result type for service operations
pub type ServiceResult<T> = Result<T, ServiceError>;
