//! Kraken Spot market implementation.
//!
//! This module provides the execution venue for Kraken Spot trading.

mod normalizer;
mod rest_client;
mod types;
mod venue;

pub use normalizer::SpotExecutionNormalizer;
pub use rest_client::SpotRestClient;
pub use types::{
    SpotAccountBalance, SpotAddOrderResponse, SpotCancelOrderResponse, SpotOrderInfo,
    SpotSystemStatus,
};
pub use venue::KrakenSpotVenue;
