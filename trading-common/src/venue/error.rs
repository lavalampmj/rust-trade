//! Venue error types with error classification for retry logic.
//!
//! This module provides a unified error type for all venue operations,
//! whether for data streaming or order execution.

use std::time::Duration;
use thiserror::Error;

use crate::error::{ErrorCategory, ErrorClassification};

/// Result type for venue operations.
pub type VenueResult<T> = Result<T, VenueError>;

/// Errors that can occur during venue operations.
///
/// This error type covers both data plane errors (streaming, historical data)
/// and execution plane errors (orders, balances).
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum VenueError {
    /// Connection error (WebSocket, TCP, etc.)
    #[error("Connection error: {0}")]
    Connection(String),

    /// Authentication error (invalid API key, signature, etc.)
    #[error("Authentication error: {0}")]
    Authentication(String),

    /// HTTP request error
    #[error("Request error: {0}")]
    Request(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded: retry after {retry_after:?}")]
    RateLimit {
        /// Suggested time to wait before retrying
        retry_after: Option<Duration>,
    },

    /// Order was rejected by the venue
    #[error("Order rejected: {reason}")]
    OrderRejected {
        /// Reason for rejection
        reason: String,
        /// Venue-specific error code
        code: Option<i32>,
    },

    /// Order not found
    #[error("Order not found: {0}")]
    OrderNotFound(String),

    /// Insufficient balance for the operation
    #[error("Insufficient balance: {0}")]
    InsufficientBalance(String),

    /// Invalid order parameters
    #[error("Invalid order: {0}")]
    InvalidOrder(String),

    /// Symbol not found or not supported
    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),

    /// Operation timed out
    #[error("Timeout: {0}")]
    Timeout(String),

    /// Failed to parse response
    #[error("Parse error: {0}")]
    Parse(String),

    /// Venue-specific error with code and message
    #[error("Venue error [{code}]: {message}")]
    VenueSpecific {
        /// Venue error code
        code: i32,
        /// Venue error message
        message: String,
    },

    /// WebSocket stream error
    #[error("Stream error: {0}")]
    Stream(String),

    /// Not connected to the venue
    #[error("Not connected")]
    NotConnected,

    /// Configuration error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Operation cancelled
    #[error("Operation cancelled: {0}")]
    Cancelled(String),

    /// Subscription error (for data streaming)
    #[error("Subscription error: {0}")]
    Subscription(String),

    /// Data not available
    #[error("Data not available: {0}")]
    DataNotAvailable(String),

    /// Internal error (unexpected state, bugs)
    #[error("Internal error: {0}")]
    Internal(String),
}

impl ErrorClassification for VenueError {
    fn category(&self) -> ErrorCategory {
        match self {
            VenueError::Connection(_) => ErrorCategory::Transient,
            VenueError::Authentication(_) => ErrorCategory::Configuration,
            VenueError::Request(_) => ErrorCategory::Transient,
            VenueError::RateLimit { .. } => ErrorCategory::ResourceExhausted,
            VenueError::OrderRejected { .. } => ErrorCategory::Permanent,
            VenueError::OrderNotFound(_) => ErrorCategory::Permanent,
            VenueError::InsufficientBalance(_) => ErrorCategory::Permanent,
            VenueError::InvalidOrder(_) => ErrorCategory::Permanent,
            VenueError::SymbolNotFound(_) => ErrorCategory::Permanent,
            VenueError::Timeout(_) => ErrorCategory::Transient,
            VenueError::Parse(_) => ErrorCategory::Permanent,
            VenueError::VenueSpecific { code, .. } => {
                // Binance-style error code classification
                match code {
                    -1015 => ErrorCategory::ResourceExhausted,
                    -1021 | -1022 => ErrorCategory::Transient,
                    -2010 | -2011 => ErrorCategory::Permanent,
                    -1099..=-1000 => ErrorCategory::Internal,
                    -1199..=-1100 => ErrorCategory::Permanent,
                    _ => ErrorCategory::Transient,
                }
            }
            VenueError::Stream(_) => ErrorCategory::Transient,
            VenueError::NotConnected => ErrorCategory::Transient,
            VenueError::Configuration(_) => ErrorCategory::Configuration,
            VenueError::Cancelled(_) => ErrorCategory::Permanent,
            VenueError::Subscription(_) => ErrorCategory::Transient,
            VenueError::DataNotAvailable(_) => ErrorCategory::Permanent,
            VenueError::Internal(_) => ErrorCategory::Internal,
        }
    }

    fn suggested_retry_delay(&self) -> Option<Duration> {
        match self {
            VenueError::RateLimit { retry_after } => {
                retry_after.or(Some(Duration::from_secs(60)))
            }
            VenueError::Connection(_) => Some(Duration::from_secs(1)),
            VenueError::Timeout(_) => Some(Duration::from_millis(500)),
            VenueError::Request(_) => Some(Duration::from_millis(100)),
            VenueError::Stream(_) => Some(Duration::from_secs(1)),
            VenueError::NotConnected => Some(Duration::from_millis(100)),
            VenueError::Subscription(_) => Some(Duration::from_millis(500)),
            VenueError::VenueSpecific { code, .. } => match code {
                -1015 => Some(Duration::from_secs(60)),
                -1021 | -1022 => Some(Duration::from_millis(100)),
                _ => None,
            },
            _ => None,
        }
    }

    fn max_retries(&self) -> u32 {
        match self.category() {
            ErrorCategory::Transient => 3,
            ErrorCategory::ResourceExhausted => 5,
            _ => 0,
        }
    }
}

impl VenueError {
    /// Create an order rejected error.
    pub fn order_rejected(reason: impl Into<String>) -> Self {
        Self::OrderRejected {
            reason: reason.into(),
            code: None,
        }
    }

    /// Create an order rejected error with a code.
    pub fn order_rejected_with_code(reason: impl Into<String>, code: i32) -> Self {
        Self::OrderRejected {
            reason: reason.into(),
            code: Some(code),
        }
    }

    /// Create a venue-specific error.
    pub fn venue_specific(code: i32, message: impl Into<String>) -> Self {
        Self::VenueSpecific {
            code,
            message: message.into(),
        }
    }

    /// Create a rate limit error with a retry delay.
    pub fn rate_limited(retry_after: Duration) -> Self {
        Self::RateLimit {
            retry_after: Some(retry_after),
        }
    }

    /// Create a rate limit error without a specific retry delay.
    pub fn rate_limited_unknown() -> Self {
        Self::RateLimit { retry_after: None }
    }

    /// Returns true if this is a rate limit error.
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, VenueError::RateLimit { .. })
    }

    /// Returns true if this is an authentication error.
    pub fn is_auth_error(&self) -> bool {
        matches!(self, VenueError::Authentication(_))
    }

    /// Returns the venue error code if available.
    pub fn error_code(&self) -> Option<i32> {
        match self {
            VenueError::OrderRejected { code, .. } => *code,
            VenueError::VenueSpecific { code, .. } => Some(*code),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_venue_error_classification() {
        let err = VenueError::Connection("refused".to_string());
        assert!(err.is_transient());
        assert!(!err.is_permanent());
        assert!(err.suggested_retry_delay().is_some());

        let err = VenueError::OrderRejected {
            reason: "Invalid quantity".to_string(),
            code: Some(-1013),
        };
        assert!(!err.is_transient());
        assert!(err.is_permanent());
    }

    #[test]
    fn test_rate_limit_error() {
        let err = VenueError::rate_limited(Duration::from_secs(30));
        assert!(err.is_rate_limited());
        assert!(err.is_transient());
        assert_eq!(err.suggested_retry_delay(), Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_venue_specific_error_classification() {
        let err = VenueError::venue_specific(-1015, "Too many requests");
        assert_eq!(err.category(), ErrorCategory::ResourceExhausted);
        assert!(err.is_transient());

        let err = VenueError::venue_specific(-1100, "Invalid parameter");
        assert_eq!(err.category(), ErrorCategory::Permanent);
        assert!(!err.is_transient());
    }

    #[test]
    fn test_data_errors() {
        let err = VenueError::Subscription("Failed to subscribe".to_string());
        assert!(err.is_transient());
        assert!(err.suggested_retry_delay().is_some());

        let err = VenueError::DataNotAvailable("No data for symbol".to_string());
        assert!(err.is_permanent());
    }
}
