//! Data provider abstractions and implementations
//!
//! This module defines the pluggable provider interface and implements
//! specific providers (Databento, Binance, mock for testing, etc.)

mod traits;
pub mod binance;
pub mod databento;
pub mod mock;

pub use traits::*;
