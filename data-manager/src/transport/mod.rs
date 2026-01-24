//! Data distribution transport layer
//!
//! This module provides transport mechanisms for distributing real-time
//! market data to consumers. The primary transport is shared memory IPC
//! for ultra-low latency within a single host.

pub mod grpc;
pub mod ipc;
mod traits;

pub use traits::*;
