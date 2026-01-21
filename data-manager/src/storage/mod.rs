//! Storage layer for market data
//!
//! This module provides TimescaleDB storage for normalized market data,
//! including batch inserts, queries, and compression management.

mod repository;
mod timescale;

pub use repository::*;
pub use timescale::*;
