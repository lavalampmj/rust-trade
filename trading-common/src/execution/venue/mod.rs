//! Execution venue infrastructure.
//!
//! This module provides the traits, types, and implementations for
//! connecting to execution venues (exchanges) for order management.
//!
//! # Migration Notice
//!
//! The core venue abstractions are being unified in the [`crate::venue`] module.
//! HTTP and WebSocket infrastructure is now available from [`crate::venue::http`]
//! and [`crate::venue::websocket`]. This module re-exports from there for
//! backward compatibility.
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
//! - [`kraken::KrakenSpotVenue`]: Kraken Spot trading (US-accessible, true order modification)
//!
//! # Example
//!
//! ```ignore
//! use trading_common::execution::venue::{
//!     ExecutionVenue, OrderSubmissionVenue,
//!     binance::create_binance_spot_us,
//!     kraken::create_kraken_spot,
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
pub mod kraken;
pub mod router;

// Re-export HTTP/WebSocket from unified venue module for new code
pub mod http {
    //! HTTP client infrastructure (re-exported from [`crate::venue::http`]).
    pub use crate::venue::http::*;
}

pub mod websocket {
    //! WebSocket client infrastructure (re-exported from [`crate::venue::websocket`]).
    pub use crate::venue::websocket::*;
}

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

// Re-export unified venue types for new code
pub use crate::venue::ConnectionStatus;
pub use crate::venue::{NativeDbVenue, SymbolNormalizer};
