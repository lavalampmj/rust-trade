//! Multi-bucket rate limiting for API requests.
//!
//! This module provides a rate limiter that supports multiple rate limit buckets:
//! - Orders per second (for order operations)
//! - Request weight per minute (for general API calls)
//!
//! Uses the `governor` crate for token bucket rate limiting.

use std::num::NonZeroU32;
use std::sync::Arc;

use governor::{
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter as GovernorRateLimiter,
};
use tracing::{debug, warn};

use crate::venue::config::RateLimitConfig;
use crate::venue::error::{VenueError, VenueResult};

type Limiter = GovernorRateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>;

/// Multi-bucket rate limiter for API requests.
///
/// Supports two types of rate limits:
/// - Order rate: Limits order submission rate (e.g., 10 orders/second)
/// - Weight rate: Limits total request weight (e.g., 1200 weight/minute)
///
/// # Example
///
/// ```ignore
/// let config = RateLimitConfig {
///     orders_per_second: 10,
///     request_weight_per_minute: 1200,
///     ..Default::default()
/// };
/// let limiter = RateLimiter::from_config(&config);
///
/// // Before submitting an order
/// limiter.check_order_rate().await?;
///
/// // Before making an API call with weight
/// limiter.check_weight_rate(5).await?;
/// ```
pub struct RateLimiter {
    /// Rate limiter for order operations
    order_limiter: Option<Arc<Limiter>>,
    /// Rate limiter for request weight
    weight_limiter: Option<Arc<Limiter>>,
    /// Configuration
    config: RateLimitConfig,
}

impl RateLimiter {
    /// Create a new rate limiter from configuration.
    pub fn from_config(config: &RateLimitConfig) -> Self {
        let order_limiter = Self::create_order_limiter(config);
        let weight_limiter = Self::create_weight_limiter(config);

        Self {
            order_limiter,
            weight_limiter,
            config: config.clone(),
        }
    }

    /// Create the order rate limiter.
    fn create_order_limiter(config: &RateLimitConfig) -> Option<Arc<Limiter>> {
        let effective_rate = config.effective_orders_per_second();
        if effective_rate == 0 {
            return None;
        }

        NonZeroU32::new(effective_rate).map(|rate| {
            let quota = Quota::per_second(rate);
            Arc::new(GovernorRateLimiter::direct(quota))
        })
    }

    /// Create the weight rate limiter.
    fn create_weight_limiter(config: &RateLimitConfig) -> Option<Arc<Limiter>> {
        let effective_weight = config.effective_request_weight_per_minute();
        if effective_weight == 0 {
            return None;
        }

        NonZeroU32::new(effective_weight).map(|rate| {
            let quota = Quota::per_minute(rate);
            Arc::new(GovernorRateLimiter::direct(quota))
        })
    }

    /// Check if an order can be submitted, waiting if necessary.
    ///
    /// This method blocks until the rate limit allows the request.
    pub async fn check_order_rate(&self) -> VenueResult<()> {
        if let Some(ref limiter) = self.order_limiter {
            debug!("Checking order rate limit");
            limiter.until_ready().await;
        }
        Ok(())
    }

    /// Check if a request with given weight can be made, waiting if necessary.
    ///
    /// # Arguments
    ///
    /// * `weight` - The weight of the request (typically 1-20 depending on endpoint)
    pub async fn check_weight_rate(&self, weight: u32) -> VenueResult<()> {
        if let Some(ref limiter) = self.weight_limiter {
            debug!("Checking weight rate limit for weight={}", weight);

            // For requests with weight > 1, we need to check multiple times
            for _ in 0..weight {
                limiter.until_ready().await;
            }
        }
        Ok(())
    }

    /// Try to acquire order rate immediately, returning error if rate limited.
    ///
    /// Use this when you want to fail fast instead of waiting.
    pub fn try_order_rate(&self) -> VenueResult<()> {
        if let Some(ref limiter) = self.order_limiter {
            match limiter.check() {
                Ok(_) => Ok(()),
                Err(not_until) => {
                    let wait_time = not_until.wait_time_from(governor::clock::Clock::now(
                        &governor::clock::DefaultClock::default(),
                    ));
                    warn!("Order rate limit exceeded, retry after {:?}", wait_time);
                    Err(VenueError::rate_limited(wait_time))
                }
            }
        } else {
            Ok(())
        }
    }

    /// Try to acquire weight rate immediately, returning error if rate limited.
    pub fn try_weight_rate(&self, weight: u32) -> VenueResult<()> {
        if let Some(ref limiter) = self.weight_limiter {
            // Check if we have enough capacity
            for _ in 0..weight {
                match limiter.check() {
                    Ok(_) => {}
                    Err(not_until) => {
                        let wait_time = not_until.wait_time_from(governor::clock::Clock::now(
                            &governor::clock::DefaultClock::default(),
                        ));
                        warn!(
                            "Weight rate limit exceeded, retry after {:?}",
                            wait_time
                        );
                        return Err(VenueError::rate_limited(wait_time));
                    }
                }
            }
        }
        Ok(())
    }

    /// Get the current rate limit configuration.
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Check if rate limiting is enabled.
    pub fn is_enabled(&self) -> bool {
        self.order_limiter.is_some() || self.weight_limiter.is_some()
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::from_config(&RateLimitConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_from_config() {
        let config = RateLimitConfig {
            orders_per_second: 10,
            request_weight_per_minute: 1200,
            buffer_factor: 0.9,
            ..Default::default()
        };
        let limiter = RateLimiter::from_config(&config);

        assert!(limiter.order_limiter.is_some());
        assert!(limiter.weight_limiter.is_some());
        assert!(limiter.is_enabled());
    }

    #[test]
    fn test_disabled_rate_limiter() {
        let config = RateLimitConfig {
            orders_per_second: 0,
            request_weight_per_minute: 0,
            ..Default::default()
        };
        let limiter = RateLimiter::from_config(&config);

        assert!(limiter.order_limiter.is_none());
        assert!(limiter.weight_limiter.is_none());
        assert!(!limiter.is_enabled());
    }

    #[tokio::test]
    async fn test_try_order_rate_success() {
        let config = RateLimitConfig {
            orders_per_second: 100, // High limit for testing
            ..Default::default()
        };
        let limiter = RateLimiter::from_config(&config);

        // Should succeed immediately
        assert!(limiter.try_order_rate().is_ok());
    }

    #[tokio::test]
    async fn test_check_order_rate() {
        let config = RateLimitConfig {
            orders_per_second: 100,
            ..Default::default()
        };
        let limiter = RateLimiter::from_config(&config);

        // Should complete without error
        limiter.check_order_rate().await.unwrap();
    }

    #[tokio::test]
    async fn test_check_weight_rate() {
        let config = RateLimitConfig {
            request_weight_per_minute: 1200,
            ..Default::default()
        };
        let limiter = RateLimiter::from_config(&config);

        // Should complete without error
        limiter.check_weight_rate(5).await.unwrap();
    }
}
