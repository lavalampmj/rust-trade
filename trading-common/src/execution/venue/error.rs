//! Venue error types (re-exports from unified venue module).
//!
//! This module re-exports error types from [`crate::venue`] for
//! backward compatibility. New code should import from `crate::venue` directly.

pub use crate::venue::{VenueError, VenueResult};
