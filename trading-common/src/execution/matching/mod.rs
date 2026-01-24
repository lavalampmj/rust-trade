//! Order matching engine for realistic trade simulation.
//!
//! This module provides order matching that works with any data granularity:
//! - Bars only (OHLC)
//! - Trade ticks
//! - L1 quotes (bid/ask)
//! - L2 order book depth
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    OrderMatchingEngine                          │
//! │                                                                  │
//! │  ┌─────────────────┐   ┌─────────────────┐   ┌──────────────┐  │
//! │  │  Trigger Orders │   │  Resting Orders │   │  OrderBook   │  │
//! │  │  (stops)        │   │  (limits)       │   │  (optional)  │  │
//! │  └────────┬────────┘   └────────┬────────┘   └──────┬───────┘  │
//! │           │                     │                    │          │
//! │           └──────────┬──────────┴────────────────────┘          │
//! │                      │                                          │
//! │                      ▼                                          │
//! │           ┌─────────────────────┐                               │
//! │           │  Price Processing   │                               │
//! │           │                     │                               │
//! │           │  • process_bar()    │──────▶ Vec<MatchResult>       │
//! │           │  • process_tick()   │                               │
//! │           │  • process_quote()  │                               │
//! │           └─────────────────────┘                               │
//! └──────────────────────────────────────────────────────────────────┘
//! ```

mod core;
mod engine;

pub use core::{MatchResult, OrderMatchingCore};
pub use engine::{MatchingEngineConfig, OrderMatchingEngine};
