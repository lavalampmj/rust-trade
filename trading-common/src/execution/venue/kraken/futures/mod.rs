//! Kraken Futures execution venue implementation.
//!
//! This module provides the `KrakenFuturesVenue` struct that implements
//! all execution venue traits for Kraken Futures (derivatives) trading.
//!
//! # Key Features
//!
//! - **True Demo Environment**: Unlike Spot, Kraken Futures has a public demo!
//! - **Perpetual Contracts**: Supports perpetual futures (e.g., PI_XBTUSD)
//! - **Leverage Support**: Configurable leverage per instrument
//! - **WebSocket V1**: Different from Spot V2 WebSocket
//!
//! # Sandbox/Demo
//!
//! Kraken Futures provides a fully functional demo environment at:
//! - REST: `https://demo-futures.kraken.com`
//! - WebSocket: `wss://demo-futures.kraken.com/ws/v1`
//!
//! # Example
//!
//! ```ignore
//! use trading_common::execution::venue::kraken::futures::KrakenFuturesVenue;
//! use trading_common::execution::venue::kraken::config::KrakenFuturesVenueConfig;
//!
//! // Create demo venue (recommended for testing)
//! let config = KrakenFuturesVenueConfig::demo();
//! let mut venue = KrakenFuturesVenue::new(config)?;
//!
//! venue.connect().await?;
//! let balances = venue.query_balances().await?;
//! ```

mod normalizer;
mod rest_client;
mod types;
mod venue;

pub use normalizer::FuturesExecutionNormalizer;
pub use rest_client::FuturesRestClient;
pub use types::*;
pub use venue::KrakenFuturesVenue;
