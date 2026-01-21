//! Common normalized data types
//!
//! This module defines the canonical data schema used throughout the data manager.
//! All provider-specific data is normalized to these types before storage or distribution.

mod market_data;
mod conversion;

pub use market_data::*;
pub use conversion::*;
