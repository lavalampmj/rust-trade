//! Subscription management
//!
//! This module handles consumer subscriptions to real-time data streams.
//! It manages which symbols each consumer is subscribed to and routes
//! data accordingly.

mod manager;
mod filter;
mod patterns;

pub use manager::*;
pub use filter::*;
pub use patterns::*;
