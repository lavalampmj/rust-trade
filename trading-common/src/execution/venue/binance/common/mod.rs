//! Common components shared between Binance Spot and Futures.
//!
//! This module provides:
//!
//! - [`BinanceHmacSigner`]: HMAC-SHA256 request signing
//! - [`ListenKeyManager`]: Listen key lifecycle management
//! - [`BinanceUserDataStream`]: User data stream WebSocket
//! - Common types (order status, side, etc.)

pub mod listen_key;
pub mod signer;
pub mod types;
pub mod user_data_stream;

pub use listen_key::ListenKeyManager;
pub use signer::BinanceHmacSigner;
pub use types::*;
pub use user_data_stream::{parse_event_type, BinanceUserDataStream, RawMessageCallback};
