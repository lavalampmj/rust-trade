//! Data provider abstractions and implementations
//!
//! This module defines the pluggable provider interface and implements
//! specific providers (Databento, Binance, Kraken, mock for testing, etc.)

pub mod binance;
pub mod databento;
pub mod kraken;
pub mod mock;
mod traits;

pub use traits::*;
