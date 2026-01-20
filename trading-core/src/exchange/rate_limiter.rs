// exchange/rate_limiter.rs
//
// Generic reconnection rate limiter for any datafeed/exchange
// Prevents excessive reconnection attempts that could lead to bans

use governor::{Quota, RateLimiter as GovernorRateLimiter, clock::DefaultClock, state::{InMemoryState, NotKeyed}};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

/// Configuration for reconnection rate limiting
#[derive(Debug, Clone)]
pub struct ReconnectionRateLimiterConfig {
    /// Maximum number of reconnection attempts allowed per time window
    pub max_attempts: u32,
    /// Time window for the rate limit (e.g., per minute, per hour)
    pub window: ReconnectionWindow,
    /// Wait duration when rate limit is exceeded (defaults to window duration)
    pub wait_on_limit_exceeded: Option<Duration>,
}

/// Time window for rate limiting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconnectionWindow {
    PerMinute,
    PerHour,
    Custom(Duration),
}

impl ReconnectionWindow {
    /// Convert window to Duration
    pub fn as_duration(&self) -> Duration {
        match self {
            ReconnectionWindow::PerMinute => Duration::from_secs(60),
            ReconnectionWindow::PerHour => Duration::from_secs(3600),
            ReconnectionWindow::Custom(d) => *d,
        }
    }
}

impl Default for ReconnectionRateLimiterConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            window: ReconnectionWindow::PerMinute,
            wait_on_limit_exceeded: None,
        }
    }
}

/// Generic reconnection rate limiter for datafeeds/exchanges
///
/// # Examples
///
/// ```
/// use trading_core::exchange::rate_limiter::{ReconnectionRateLimiter, ReconnectionRateLimiterConfig, ReconnectionWindow};
///
/// let config = ReconnectionRateLimiterConfig {
///     max_attempts: 5,
///     window: ReconnectionWindow::PerMinute,
///     wait_on_limit_exceeded: None,
/// };
///
/// let limiter = ReconnectionRateLimiter::new(config);
///
/// // Check if reconnection is allowed
/// if limiter.check_allowed() {
///     // Proceed with reconnection
/// } else {
///     // Wait before retrying
/// }
/// ```
pub struct ReconnectionRateLimiter {
    limiter: Arc<GovernorRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    config: ReconnectionRateLimiterConfig,
}

impl ReconnectionRateLimiter {
    /// Create a new rate limiter with the given configuration
    pub fn new(config: ReconnectionRateLimiterConfig) -> Self {
        let quota = match config.window {
            ReconnectionWindow::PerMinute => {
                Quota::per_minute(NonZeroU32::new(config.max_attempts).expect("max_attempts must be > 0"))
            }
            ReconnectionWindow::PerHour => {
                Quota::per_hour(NonZeroU32::new(config.max_attempts).expect("max_attempts must be > 0"))
            }
            ReconnectionWindow::Custom(duration) => {
                Quota::with_period(duration)
                    .expect("Invalid quota period")
                    .allow_burst(NonZeroU32::new(config.max_attempts).expect("max_attempts must be > 0"))
            }
        };

        let limiter = Arc::new(GovernorRateLimiter::direct(quota));

        Self { limiter, config }
    }

    /// Check if a reconnection attempt is allowed
    ///
    /// Returns `true` if the attempt is within the rate limit, `false` otherwise
    pub fn check_allowed(&self) -> bool {
        self.limiter.check().is_ok()
    }

    /// Get the wait duration when rate limit is exceeded
    pub fn wait_duration(&self) -> Duration {
        self.config
            .wait_on_limit_exceeded
            .unwrap_or_else(|| self.config.window.as_duration())
    }

    /// Get the maximum attempts configured
    pub fn max_attempts(&self) -> u32 {
        self.config.max_attempts
    }
}

/// Test-only methods
#[cfg(test)]
impl ReconnectionRateLimiter {
    /// Get the time window configured (test only)
    pub fn window(&self) -> ReconnectionWindow {
        self.config.window
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_rate_limiter_allows_initial_attempts() {
        let config = ReconnectionRateLimiterConfig {
            max_attempts: 3,
            window: ReconnectionWindow::PerMinute,
            wait_on_limit_exceeded: None,
        };

        let limiter = ReconnectionRateLimiter::new(config);

        // First 3 attempts should be allowed
        assert!(limiter.check_allowed(), "First attempt should be allowed");
        assert!(limiter.check_allowed(), "Second attempt should be allowed");
        assert!(limiter.check_allowed(), "Third attempt should be allowed");
    }

    #[test]
    fn test_rate_limiter_blocks_excessive_attempts() {
        let config = ReconnectionRateLimiterConfig {
            max_attempts: 2,
            window: ReconnectionWindow::PerMinute,
            wait_on_limit_exceeded: None,
        };

        let limiter = ReconnectionRateLimiter::new(config);

        // First 2 attempts allowed
        assert!(limiter.check_allowed());
        assert!(limiter.check_allowed());

        // Third attempt should be blocked
        assert!(!limiter.check_allowed(), "Third attempt should be blocked");
    }

    #[test]
    fn test_rate_limiter_resets_after_window() {
        let config = ReconnectionRateLimiterConfig {
            max_attempts: 2,
            window: ReconnectionWindow::Custom(Duration::from_millis(100)),
            wait_on_limit_exceeded: None,
        };

        let limiter = ReconnectionRateLimiter::new(config);

        // Use up the quota
        assert!(limiter.check_allowed());
        assert!(limiter.check_allowed());
        assert!(!limiter.check_allowed());

        // Wait for window to reset
        sleep(Duration::from_millis(150));

        // Should be allowed again
        assert!(limiter.check_allowed(), "Rate limiter should reset after window");
    }

    #[test]
    fn test_default_config() {
        let config = ReconnectionRateLimiterConfig::default();

        assert_eq!(config.max_attempts, 5);
        assert_eq!(config.window, ReconnectionWindow::PerMinute);
        assert_eq!(config.wait_on_limit_exceeded, None);
    }

    #[test]
    fn test_custom_wait_duration() {
        let custom_wait = Duration::from_secs(30);
        let config = ReconnectionRateLimiterConfig {
            max_attempts: 5,
            window: ReconnectionWindow::PerMinute,
            wait_on_limit_exceeded: Some(custom_wait),
        };

        let limiter = ReconnectionRateLimiter::new(config);

        assert_eq!(limiter.wait_duration(), custom_wait);
    }

    #[test]
    fn test_default_wait_duration_matches_window() {
        let config = ReconnectionRateLimiterConfig {
            max_attempts: 5,
            window: ReconnectionWindow::PerMinute,
            wait_on_limit_exceeded: None,
        };

        let limiter = ReconnectionRateLimiter::new(config);

        assert_eq!(limiter.wait_duration(), Duration::from_secs(60));
    }

    #[test]
    fn test_per_hour_window() {
        let config = ReconnectionRateLimiterConfig {
            max_attempts: 10,
            window: ReconnectionWindow::PerHour,
            wait_on_limit_exceeded: None,
        };

        let limiter = ReconnectionRateLimiter::new(config);

        assert_eq!(limiter.max_attempts(), 10);
        assert_eq!(limiter.window(), ReconnectionWindow::PerHour);
        assert_eq!(limiter.wait_duration(), Duration::from_secs(3600));
    }

    #[test]
    fn test_custom_window() {
        let custom_duration = Duration::from_secs(300); // 5 minutes
        let config = ReconnectionRateLimiterConfig {
            max_attempts: 3,
            window: ReconnectionWindow::Custom(custom_duration),
            wait_on_limit_exceeded: None,
        };

        let limiter = ReconnectionRateLimiter::new(config);

        assert_eq!(limiter.window(), ReconnectionWindow::Custom(custom_duration));
        assert_eq!(limiter.wait_duration(), custom_duration);
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let config = ReconnectionRateLimiterConfig {
            max_attempts: 5,
            window: ReconnectionWindow::Custom(Duration::from_secs(1)),
            wait_on_limit_exceeded: None,
        };

        let limiter = Arc::new(ReconnectionRateLimiter::new(config));

        let mut handles = vec![];

        // Spawn multiple threads trying to check the rate limiter
        for _ in 0..10 {
            let limiter_clone = Arc::clone(&limiter);
            let handle = thread::spawn(move || {
                limiter_clone.check_allowed()
            });
            handles.push(handle);
        }

        let mut allowed_count = 0;
        for handle in handles {
            if handle.join().unwrap() {
                allowed_count += 1;
            }
        }

        // Should allow exactly 5 (max_attempts)
        assert_eq!(allowed_count, 5, "Should allow exactly max_attempts connections");
    }
}
