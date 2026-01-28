//! Instrument Management
//!
//! This module provides instrument identification and metadata management
//! across all data providers.
//!
//! # Components
//!
//! - [`InstrumentRegistry`]: Persistent instrument_id assignment for all providers
//!
//! # Example
//!
//! ```ignore
//! use data_manager::instruments::InstrumentRegistry;
//!
//! // Create registry with database connection
//! let registry = InstrumentRegistry::new(pool).await?;
//!
//! // Get or create instrument_id (thread-safe, cached)
//! let id = registry.get_or_create("BTCUSD", "BINANCE").await?;
//! ```

mod registry;

pub use registry::{InstrumentMapping, InstrumentRegistry, RegistryError, RegistryResult, RegistryStats};
