//! Unified venue trait architecture for data ingestion and order execution.
//!
//! This module provides a unified foundation for connecting to trading venues,
//! whether for data streaming or order execution. The key abstractions are:
//!
//! # Core Traits
//!
//! - [`VenueConnection`]: Base trait for all venue connections (lifecycle, status)
//! - [`SymbolNormalizer`]: Symbol format conversion between venue and canonical formats
//!
//! # Data Plane Traits
//!
//! - [`DataStreamVenue`]: Real-time data streaming (trades, quotes, orderbook)
//! - [`HistoricalDataVenue`]: Historical data retrieval for backtesting
//! - [`FullDataVenue`]: Combined trait for full-featured data venues
//!
//! # Execution Plane Traits
//!
//! - [`OrderSubmissionVenue`]: Order lifecycle management via REST API
//! - [`ExecutionStreamVenue`]: Real-time execution reports via WebSocket
//! - [`AccountQueryVenue`]: Account balance and position queries
//! - [`FullExecutionVenue`]: Combined trait for full-featured execution venues
//!
//! # Shared Infrastructure
//!
//! - [`http`]: Authenticated HTTP client with rate limiting
//! - [`websocket`]: Reconnecting WebSocket client
//!
//! # Example
//!
//! ```ignore
//! use trading_common::venue::{
//!     VenueConnection, SymbolNormalizer,
//!     data::DataStreamVenue,
//!     execution::OrderSubmissionVenue,
//! };
//!
//! // Venues can implement both data and execution traits
//! async fn example<V>(venue: &mut V)
//! where
//!     V: VenueConnection + DataStreamVenue + OrderSubmissionVenue,
//! {
//!     // Connect to the venue
//!     venue.connect().await?;
//!
//!     // Convert symbols
//!     let venue_symbol = venue.to_venue("BTCUSD")?;
//!
//!     // Use both data and execution capabilities
//! }
//! ```

mod config;
mod connection;
mod error;
mod symbology;
mod types;

pub mod data;
pub mod execution;
pub mod http;
pub mod websocket;

// Re-export core types
pub use config::{AuthConfig, RateLimitConfig, RestConfig, StreamConfig, VenueConfig};
pub use connection::{ConnectionStatus, VenueConnection};
pub use error::{VenueError, VenueResult};
pub use symbology::{NativeDbVenue, SymbolNormalizer};
pub use types::{VenueCapabilities, VenueInfo};

// Re-export data traits
pub use data::{DataStreamVenue, FullDataVenue, HistoricalDataVenue};

// Re-export execution traits
pub use execution::{
    AccountQueryVenue, BatchOrderVenue, ExecutionCallback, ExecutionNormalizer,
    ExecutionStreamVenue, FullExecutionVenue, OrderSubmissionVenue,
};

// Re-export common execution types
pub use execution::{
    BalanceInfo, BatchCancelResult, BatchOrderResult, CancelRequest, ExecutionReport,
    OrderQueryResponse,
};
