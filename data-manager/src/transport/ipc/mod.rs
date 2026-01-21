//! Shared memory IPC transport
//!
//! This module implements ultra-low latency data distribution using
//! lock-free ring buffers in shared memory.

mod ring_buffer;
mod shared_memory;
mod consumer;

pub use ring_buffer::*;
pub use shared_memory::*;
pub use consumer::*;
