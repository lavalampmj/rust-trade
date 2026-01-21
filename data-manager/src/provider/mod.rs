//! Data provider abstractions and implementations
//!
//! This module defines the pluggable provider interface and implements
//! specific providers (Databento, mock for testing, etc.)

mod traits;
pub mod databento;
pub mod mock;

pub use traits::*;
