//! Validation utilities for trading data.
//!
//! Provides reusable validators for symbols, exchanges, and other trading entities.

mod symbol;

pub use symbol::{SymbolValidationError, SymbolValidator, SymbolValidatorConfig};
