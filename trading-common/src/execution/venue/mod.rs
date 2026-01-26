//! Execution venue infrastructure.
//!
//! This module provides the traits, types, and implementations for
//! connecting to execution venues (exchanges) for order management.
//!
//! # Core Abstractions
//!
//! - **Traits**: [`ExecutionVenue`], [`OrderSubmissionVenue`], [`ExecutionStreamVenue`], [`AccountQueryVenue`]
//! - **Types**: [`VenueInfo`], [`ExecutionReport`], [`BalanceInfo`], etc.
//! - **Errors**: [`VenueError`] with error classification for retry logic
//! - **Config**: Venue configuration types for TOML deserialization
//!
//! # HTTP/WebSocket Infrastructure
//!
//! - [`http::HttpClient`]: Authenticated HTTP client with rate limiting
//! - [`http::RequestSigner`]: Trait for venue-specific request signing
//! - [`http::RateLimiter`]: Multi-bucket rate limiter
//! - [`websocket::WebSocketClient`]: Reconnecting WebSocket client
//!
//! # Venue Implementations
//!
//! - [`binance::BinanceSpotVenue`]: Binance.com/US Spot trading
//! - [`binance::BinanceFuturesVenue`]: Binance USDT-M Futures
//!
//! # Example
//!
//! ```ignore
//! use trading_common::execution::venue::{
//!     ExecutionVenue, OrderSubmissionVenue,
//!     binance::create_binance_spot_us,
//! };
//! use trading_common::orders::Order;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create and connect to venue
//!     let mut venue = create_binance_spot_us()?;
//!     venue.connect().await?;
//!
//!     // Query balances
//!     let balances = venue.query_balances().await?;
//!     println!("Balances: {:?}", balances);
//!
//!     // Submit an order
//!     let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(0.001)).build()?;
//!     let venue_order_id = venue.submit_order(&order).await?;
//!
//!     Ok(())
//! }
//! ```

mod config;
mod error;
mod normalizer;
mod traits;
mod types;

pub mod binance;
pub mod http;
pub mod websocket;

// Re-export core types
pub use config::{AuthConfig, RateLimitConfig, RestConfig, StreamConfig, VenueConfig};
pub use error::{VenueError, VenueResult};
pub use normalizer::ExecutionNormalizer;
pub use traits::{
    AccountQueryVenue, BatchOrderVenue, ExecutionCallback, ExecutionStreamVenue, ExecutionVenue,
    FullExecutionVenue, OrderSubmissionVenue,
};
pub use types::{
    BalanceInfo, BatchCancelResult, BatchOrderResult, CancelRequest, ExecutionReport,
    OrderQueryResponse, VenueConnectionStatus, VenueInfo,
};
