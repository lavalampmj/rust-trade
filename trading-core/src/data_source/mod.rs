//! Data Source Module
//!
//! This module provides a unified interface for receiving market data.
//! Supports both IPC (shared memory) from data-manager and direct exchange connections.

mod ipc;
mod ipc_exchange;

pub use ipc::*;
pub use ipc_exchange::*;
