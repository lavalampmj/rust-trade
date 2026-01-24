//! Subscription management
//!
//! This module handles consumer subscriptions to real-time data streams.
//! It manages which symbols each consumer is subscribed to and routes
//! data accordingly.

mod filter;
mod manager;
mod patterns;

pub use filter::*;
pub use manager::*;
pub use patterns::*;
