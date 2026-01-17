// exchange/mod.rs
pub mod binance;
pub mod errors;
pub mod rate_limiter;
pub mod traits;
pub mod types;
pub mod utils;

// Re-export main interfaces for easy access
pub use binance::BinanceExchange;
pub use errors::ExchangeError;
pub use rate_limiter::{ReconnectionRateLimiter, ReconnectionRateLimiterConfig, ReconnectionWindow};
pub use traits::Exchange;
pub use types::*;
