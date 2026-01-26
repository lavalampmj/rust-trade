//! Common Kraken components shared between spot and futures.
//!
//! This module contains:
//! - [`KrakenHmacSigner`]: HMAC-SHA512 request signer
//! - [`KrakenUserStream`]: WebSocket execution stream handler
//! - Shared types and utilities

mod signer;
pub mod types;
mod user_stream;
mod ws_token;

pub use signer::KrakenHmacSigner;
pub use types::{
    KrakenExecutionType, KrakenLiquiditySide, KrakenOrderSide, KrakenOrderStatus,
    KrakenOrderType, KrakenResponse, KrakenTimeInForce, parse_kraken_decimal, timestamp_to_datetime,
};
pub use user_stream::KrakenUserStream;
pub use ws_token::KrakenWsTokenManager;

/// Callback type for raw WebSocket messages.
pub type RawMessageCallback = std::sync::Arc<dyn Fn(String) + Send + Sync>;
