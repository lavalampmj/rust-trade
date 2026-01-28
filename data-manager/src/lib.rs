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
pub mod cli;
pub mod config;
pub mod instruments;
pub mod provider;
pub mod scheduler;
pub mod schema;
pub mod storage;
pub mod subscription;
pub mod symbol;
pub mod transport;
pub mod validation;

// Re-export commonly used types
pub use config::Settings;
pub use instruments::{InstrumentRegistry, RegistryError};
pub use provider::{
    DataProvider, HistoricalDataProvider, LiveStreamProvider, ProviderError, ProviderInfo,
    ProviderResult,
};
pub use schema::{NormalizedOHLC, NormalizedTick, TradeSide};
pub use storage::MarketDataRepository;
pub use subscription::{BackfillRequest, SubscriptionManager, SubscriptionRequest};
pub use symbol::{SymbolRegistry, SymbolSpec, SymbolUniverse};
pub use transport::{Transport, TransportError};
pub use validation::{TickValidator, ValidationConfig, ValidationError, ValidationResult};
