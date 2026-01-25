//! Consolidated error handling module for the trading system.
//!
//! This module provides:
//! - Common error types that can be reused across crates
//! - Error classification traits for retry logic
//! - Standardized error patterns
//!
//! # Usage
//!
//! ```rust,ignore
//! use trading_common::error::{CommonError, ErrorClassification};
//!
//! fn handle_error(err: impl ErrorClassification) {
//!     if err.is_transient() {
//!         if let Some(delay) = err.suggested_retry_delay() {
//!             // Retry after delay
//!         }
//!     }
//! }
//! ```

mod common;
mod traits;

pub use common::*;
pub use traits::*;
