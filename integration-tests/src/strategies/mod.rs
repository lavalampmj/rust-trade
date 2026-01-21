//! Test strategies for the integration test harness
//!
//! This module contains strategy implementations used for testing the data pipeline.
//! Strategies are minimal implementations focused on tick counting and latency measurement.

mod test_tick_counter;

pub use test_tick_counter::TestTickCounterStrategy;
