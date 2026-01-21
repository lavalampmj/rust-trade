//! Data Source Module
//!
//! This module provides data sources for receiving market data.
//! Currently supports IPC (shared memory) from data-manager.

mod ipc;

pub use ipc::*;
