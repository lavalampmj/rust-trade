//! # Data Manager
//!
//! Centralized data infrastructure for market data loading, streaming, and distribution.
//!
//! ## Features
//!
//! - **Historical data loading**: On-demand, scheduled bulk import, third-party vendors
//! - **Real-time streaming**: Subscribe to live data from providers (20K+ symbols)
//! - **Data distribution**: Shared memory IPC for <10ms latency to consumers
//! - **Automatic backfill**: Gap detection and cost-controlled data backfill
//!
//! ## Architecture
//!
//! The data manager uses a pluggable provider architecture with Databento as the initial
//! implementation. Data is normalized to a common schema and stored in TimescaleDB for
//! historical access. Real-time data is distributed via shared memory ring buffers for
//! ultra-low latency.

pub mod backfill;
pub mod config;
pub mod provider;
pub mod schema;
pub mod storage;
pub mod subscription;
pub mod symbol;
pub mod transport;
pub mod scheduler;
pub mod cli;

// Re-export commonly used types
pub use config::Settings;
pub use provider::{
    DataProvider, HistoricalDataProvider, LiveStreamProvider,
    ProviderError, ProviderResult, ProviderInfo,
};
pub use schema::{NormalizedTick, NormalizedOHLC, TradeSide};
pub use storage::MarketDataRepository;
pub use subscription::{SubscriptionManager, SubscriptionRequest, BackfillRequest};
pub use symbol::{SymbolUniverse, SymbolSpec, SymbolRegistry};
pub use transport::{Transport, TransportError};
