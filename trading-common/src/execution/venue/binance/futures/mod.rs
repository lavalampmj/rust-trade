//! Binance USDT-M Futures market implementation.
//!
//! This module provides the execution venue implementation for Binance Futures:
//!
//! - [`BinanceFuturesVenue`]: Main venue implementing all execution traits + futures-specific operations
//! - [`FuturesRestClient`]: REST API client for order and position operations
//! - [`FuturesExecutionNormalizer`]: Converts Binance types to venue-agnostic types
//! - Futures-specific types (positions, leverage, margin, etc.)
//!
//! # Futures-Specific Features
//!
//! - Leverage management (set/get per symbol)
//! - Margin type (cross/isolated)
//! - Position mode (hedge/one-way)
//! - Position queries
//! - Position side support for hedge mode

pub mod normalizer;
pub mod rest_client;
pub mod types;
pub mod venue;

pub use normalizer::FuturesExecutionNormalizer;
pub use rest_client::FuturesRestClient;
pub use types::*;
pub use venue::{BinanceFuturesVenue, FuturesPositionInfo, MarginType};
