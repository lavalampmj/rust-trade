//! Storage layer for market data
//!
//! This module provides TimescaleDB storage for normalized market data,
//! including batch inserts, queries, and compression management.

mod ohlc;
mod repository;
mod timescale;

pub use ohlc::*;
pub use repository::*;
pub use timescale::*;
