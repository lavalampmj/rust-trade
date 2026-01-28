//! Shared memory IPC transport
//!
//! This module implements ultra-low latency data distribution using
//! lock-free ring buffers in shared memory.

mod consumer;
mod control;
mod registry;
mod ring_buffer;
mod shared_memory;

pub use consumer::*;
pub use control::*;
pub use registry::*;
pub use ring_buffer::*;
pub use shared_memory::*;
