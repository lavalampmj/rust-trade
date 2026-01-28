//! Data provider abstractions and implementations
//!
//! This module defines the pluggable provider interface and implements
//! specific providers (Databento, Binance, Kraken, mock for testing, etc.)

pub mod binance;
pub mod databento;
pub mod factory;
pub mod kraken;
pub mod mock;
mod traits;

pub use factory::{infer_asset_type, ProviderFactory};
pub use traits::*;
