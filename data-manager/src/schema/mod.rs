//! Common normalized data types
//!
//! This module defines the canonical data schema used throughout the data manager.
//! All provider-specific data is normalized to these types before storage or distribution.

mod conversion;
mod market_data;

pub use conversion::*;
pub use market_data::*;
