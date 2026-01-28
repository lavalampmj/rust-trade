//! Storage layer for market data
//!
//! This module provides TimescaleDB storage for normalized market data,
//! including batch inserts, queries, and compression management.

mod databento_instruments;
mod ohlc;
mod repository;
mod timescale;

pub use databento_instruments::*;
pub use ohlc::*;
pub use repository::*;
pub use timescale::*;
