//! Tick data validation for market data integrity
//!
//! This module provides validation for incoming tick data at the data ingestion point.
//! Validation runs before data is stored or distributed via IPC.

mod validator;

#[cfg(test)]
mod tests;

pub use validator::{TickValidator, ValidationConfig, ValidationError, ValidationResult};
