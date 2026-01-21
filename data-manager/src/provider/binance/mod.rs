//! Binance data provider implementation
//!
//! This module implements the LiveStreamProvider trait for Binance,
//! supporting live WebSocket streaming of trade data.

mod client;
mod normalizer;
mod types;

pub use client::{BinanceProvider, BinanceSettings};
pub use normalizer::BinanceNormalizer;
