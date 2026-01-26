//! Binance Spot market implementation.
//!
//! This module provides the execution venue implementation for Binance Spot:
//!
//! - [`BinanceSpotVenue`]: Main venue implementing all execution traits
//! - [`SpotRestClient`]: REST API client for order operations
//! - [`SpotExecutionNormalizer`]: Converts Binance types to venue-agnostic types
//! - Spot-specific types (order types, responses, etc.)

pub mod normalizer;
pub mod rest_client;
pub mod types;
pub mod venue;

pub use normalizer::SpotExecutionNormalizer;
pub use rest_client::SpotRestClient;
pub use types::*;
pub use venue::BinanceSpotVenue;
