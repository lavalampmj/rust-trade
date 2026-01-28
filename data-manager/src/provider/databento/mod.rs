//! Databento data provider implementation
//!
//! This module implements the DataProvider traits for Databento,
//! supporting both historical data fetching and live streaming.
//!
//! # Key Components
//!
//! - [`DatabentoClient`]: Main client for historical and live data
//! - [`DatabentoNormalizer`]: Converts DBN messages to normalized types
//! - [`InstrumentDefinitionService`]: Resolves symbols to canonical instrument_ids
//!
//! # Instrument ID Resolution
//!
//! Databento assigns a globally unique, opaque `instrument_id` to every instrument
//! in their security master. This ID is:
//!
//! - **Stable forever**: Never changes for a given instrument
//! - **Unique across all datasets**: No collisions between venues
//! - **Opaque**: You cannot infer meaning from the number
//! - **Never reused**: Even if an instrument delists
//!
//! Use [`InstrumentDefinitionService`] to resolve symbols:
//!
//! ```ignore
//! let service = InstrumentDefinitionService::new(api_key, Some(pool));
//! let id = service.resolve("ESH6", "GLBX.MDP3").await?;
//! ```

mod client;
mod historical;
mod live;
mod metadata;
mod normalizer;

pub use client::DatabentoClient;
pub use metadata::{
    CachedInstrumentDef, CacheStats, InstrumentDefinitionService, InstrumentError,
    InstrumentResult,
};
pub use normalizer::DatabentoNormalizer;
