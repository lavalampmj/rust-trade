//! Binance data provider implementation
//!
//! This module implements the LiveStreamProvider trait for Binance,
//! supporting live WebSocket streaming of trade data.
//!
//! # Symbology
//!
//! Binance uses pairs like `BTCUSDT`, `ETHBUSD`. Internally, the framework
//! uses DBT canonical format (`BTCUSD`, `ETHUSD`). Symbol conversion happens
//! at the edge:
//!
//! - CLI/Config input (canonical) → `to_binance()` → WebSocket subscription
//! - WebSocket data → `to_canonical()` → Internal processing
//!
//! See [`symbol`] module for conversion functions.

mod client;
mod normalizer;
pub mod symbol;
mod types;

pub use client::{BinanceProvider, BinanceSettings};
pub use normalizer::BinanceNormalizer;
