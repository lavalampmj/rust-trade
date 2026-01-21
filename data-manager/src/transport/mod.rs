//! Data distribution transport layer
//!
//! This module provides transport mechanisms for distributing real-time
//! market data to consumers. The primary transport is shared memory IPC
//! for ultra-low latency within a single host.

mod traits;
pub mod ipc;
pub mod grpc;

pub use traits::*;
