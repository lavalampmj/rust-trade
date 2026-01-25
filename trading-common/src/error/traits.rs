//! Error classification traits for retry logic and error handling.
//!
//! These traits allow errors to self-describe their characteristics,
//! enabling generic retry logic and error handling patterns.

use std::time::Duration;

use super::common::*;

/// Classification of error types for handling decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Transient errors that may resolve on retry (network issues, timeouts)
    Transient,
    /// Permanent errors that won't resolve on retry (invalid input, not found)
    Permanent,
    /// Resource exhaustion errors (rate limits, pool exhausted)
    ResourceExhausted,
    /// Configuration errors (missing config, invalid settings)
    Configuration,
    /// Internal errors (bugs, unexpected state)
    Internal,
}

/// Trait for errors that can classify themselves for retry logic.
///
/// # Example
///
/// ```rust,ignore
/// use trading_common::error::{ErrorClassification, ErrorCategory};
/// use std::time::Duration;
///
/// async fn with_retry<T, E, F, Fut>(mut f: F) -> Result<T, E>
/// where
///     E: ErrorClassification,
///     F: FnMut() -> Fut,
///     Fut: std::future::Future<Output = Result<T, E>>,
/// {
///     let mut attempts = 0;
///     loop {
///         match f().await {
///             Ok(v) => return Ok(v),
///             Err(e) if e.is_transient() && attempts < 3 => {
///                 if let Some(delay) = e.suggested_retry_delay() {
///                     tokio::time::sleep(delay).await;
///                 }
///                 attempts += 1;
///             }
///             Err(e) => return Err(e),
///         }
///     }
/// }
/// ```
pub trait ErrorClassification {
    /// Returns the category of this error
    fn category(&self) -> ErrorCategory;

    /// Returns true if this error is transient and may succeed on retry
    fn is_transient(&self) -> bool {
        matches!(
            self.category(),
            ErrorCategory::Transient | ErrorCategory::ResourceExhausted
        )
    }

    /// Returns true if this error is permanent and won't succeed on retry
    fn is_permanent(&self) -> bool {
        matches!(self.category(), ErrorCategory::Permanent)
    }

    /// Suggests a delay before retrying, if applicable
    fn suggested_retry_delay(&self) -> Option<Duration> {
        match self.category() {
            ErrorCategory::Transient => Some(Duration::from_millis(100)),
            ErrorCategory::ResourceExhausted => Some(Duration::from_secs(1)),
            _ => None,
        }
    }

    /// Returns the maximum number of retries suggested for this error
    fn max_retries(&self) -> u32 {
        match self.category() {
            ErrorCategory::Transient => 3,
            ErrorCategory::ResourceExhausted => 5,
            _ => 0,
        }
    }
}

// Implement ErrorClassification for common error types

impl ErrorClassification for DatabaseError {
    fn category(&self) -> ErrorCategory {
        match self {
            DatabaseError::Connection(_) => ErrorCategory::Transient,
            DatabaseError::PoolExhausted => ErrorCategory::ResourceExhausted,
            DatabaseError::Timeout(_) => ErrorCategory::Transient,
            DatabaseError::Query(_) => ErrorCategory::Permanent, // Usually bad SQL
            DatabaseError::Transaction(_) => ErrorCategory::Transient, // May be deadlock
        }
    }

    fn suggested_retry_delay(&self) -> Option<Duration> {
        match self {
            DatabaseError::PoolExhausted => Some(Duration::from_millis(500)),
            DatabaseError::Timeout(_) => Some(Duration::from_millis(100)),
            DatabaseError::Connection(_) => Some(Duration::from_secs(1)),
            DatabaseError::Transaction(_) => Some(Duration::from_millis(50)),
            _ => None,
        }
    }
}

impl ErrorClassification for NetworkError {
    fn category(&self) -> ErrorCategory {
        match self {
            NetworkError::Connection(_) => ErrorCategory::Transient,
            NetworkError::Timeout(_) => ErrorCategory::Transient,
            NetworkError::Request(_) => ErrorCategory::Transient,
            NetworkError::Send(_) => ErrorCategory::Transient,
            NetworkError::Receive(_) => ErrorCategory::Transient,
            NetworkError::Closed(_) => ErrorCategory::Transient,
            NetworkError::DnsError(_) => ErrorCategory::Configuration, // Usually config issue
        }
    }

    fn suggested_retry_delay(&self) -> Option<Duration> {
        match self {
            NetworkError::Timeout(_) => Some(Duration::from_millis(500)),
            NetworkError::Connection(_) => Some(Duration::from_secs(1)),
            NetworkError::Closed(_) => Some(Duration::from_millis(100)),
            _ => Some(Duration::from_millis(100)),
        }
    }
}

impl ErrorClassification for ConfigurationError {
    fn category(&self) -> ErrorCategory {
        ErrorCategory::Configuration
    }

    fn suggested_retry_delay(&self) -> Option<Duration> {
        None // Configuration errors don't resolve on retry
    }

    fn max_retries(&self) -> u32 {
        0
    }
}

impl ErrorClassification for EntityError {
    fn category(&self) -> ErrorCategory {
        match self {
            EntityError::NotFound { .. } => ErrorCategory::Permanent,
            EntityError::AlreadyExists { .. } => ErrorCategory::Permanent,
            EntityError::Invalid { .. } => ErrorCategory::Permanent,
            EntityError::InvalidState { .. } => ErrorCategory::Permanent,
        }
    }
}

impl ErrorClassification for SerializationError {
    fn category(&self) -> ErrorCategory {
        ErrorCategory::Permanent // Bad data won't change on retry
    }
}

impl ErrorClassification for CacheError {
    fn category(&self) -> ErrorCategory {
        match self {
            CacheError::Connection(_) => ErrorCategory::Transient,
            CacheError::Read(_) => ErrorCategory::Transient,
            CacheError::Write(_) => ErrorCategory::Transient,
            CacheError::NotFound(_) => ErrorCategory::Permanent,
            CacheError::Serialization(_) => ErrorCategory::Permanent,
            CacheError::Unavailable => ErrorCategory::Transient,
        }
    }

    fn suggested_retry_delay(&self) -> Option<Duration> {
        match self {
            CacheError::Connection(_) => Some(Duration::from_secs(1)),
            CacheError::Unavailable => Some(Duration::from_millis(500)),
            CacheError::Read(_) | CacheError::Write(_) => Some(Duration::from_millis(100)),
            _ => None,
        }
    }
}

impl ErrorClassification for ChannelError {
    fn category(&self) -> ErrorCategory {
        match self {
            ChannelError::Closed => ErrorCategory::Permanent,
            ChannelError::SendFailed(_) => ErrorCategory::Permanent,
            ChannelError::ReceiveFailed(_) => ErrorCategory::Permanent,
            ChannelError::Full { .. } => ErrorCategory::ResourceExhausted,
            ChannelError::Timeout => ErrorCategory::Transient,
        }
    }

    fn suggested_retry_delay(&self) -> Option<Duration> {
        match self {
            ChannelError::Full { .. } => Some(Duration::from_millis(10)),
            ChannelError::Timeout => Some(Duration::from_millis(50)),
            _ => None,
        }
    }
}

impl ErrorClassification for ValidationError {
    fn category(&self) -> ErrorCategory {
        ErrorCategory::Permanent // Validation errors are input issues
    }
}

/// Helper function for retry logic
pub async fn retry_with_backoff<T, E, F, Fut>(
    mut operation: F,
    max_attempts: u32,
    initial_delay: Duration,
) -> Result<T, E>
where
    E: ErrorClassification + std::fmt::Debug,
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let mut attempts = 0;
    let mut delay = initial_delay;

    loop {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(err) => {
                attempts += 1;

                if !err.is_transient() || attempts >= max_attempts {
                    return Err(err);
                }

                let retry_delay = err.suggested_retry_delay().unwrap_or(delay);
                tokio::time::sleep(retry_delay).await;

                // Exponential backoff with cap
                delay = std::cmp::min(delay * 2, Duration::from_secs(30));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_error_classification() {
        let err = DatabaseError::Connection("refused".to_string());
        assert!(err.is_transient());
        assert!(!err.is_permanent());
        assert!(err.suggested_retry_delay().is_some());

        let err = DatabaseError::Query("syntax error".to_string());
        assert!(!err.is_transient());
        assert!(err.is_permanent());
    }

    #[test]
    fn test_network_error_classification() {
        let err = NetworkError::Timeout(Duration::from_secs(30));
        assert!(err.is_transient());
        assert_eq!(err.category(), ErrorCategory::Transient);

        let err = NetworkError::DnsError("unknown host".to_string());
        assert_eq!(err.category(), ErrorCategory::Configuration);
    }

    #[test]
    fn test_entity_error_classification() {
        let err = EntityError::not_found("Order", "123");
        assert!(err.is_permanent());
        assert_eq!(err.max_retries(), 0);
    }

    #[test]
    fn test_channel_error_classification() {
        let err = ChannelError::Full { capacity: 1000 };
        assert!(err.is_transient()); // ResourceExhausted is considered transient
        assert_eq!(err.category(), ErrorCategory::ResourceExhausted);

        let err = ChannelError::Closed;
        assert!(err.is_permanent());
    }

    #[test]
    fn test_cache_error_classification() {
        let err = CacheError::Unavailable;
        assert!(err.is_transient());

        let err = CacheError::NotFound("key".to_string());
        assert!(err.is_permanent());
    }
}
