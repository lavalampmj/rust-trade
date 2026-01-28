//! Configuration types for execution venues (re-exports from unified venue module).
//!
//! This module re-exports configuration types from [`crate::venue`] for
//! backward compatibility. New code should import from `crate::venue` directly.

pub use crate::venue::{AuthConfig, RateLimitConfig, RestConfig, StreamConfig, VenueConfig};
