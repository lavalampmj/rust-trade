//! Standardized logging configuration for the trading system.
//!
//! Provides consistent logging format across all crates with support for:
//! - Human-readable console output (default)
//! - JSON format for HTML clients and log aggregation
//!
//! # Environment Variables
//!
//! - `RUST_LOG`: Standard tracing filter (e.g., `info`, `trading_core=debug`)
//! - `LOG_FORMAT`: Output format - `pretty` (default), `compact`, or `json`
//! - `LOG_TIMESTAMPS`: Timestamp format - `local` (default), `utc`, or `none`
//!
//! # Usage
//!
//! ```rust,ignore
//! use trading_common::logging::{init_logging, LogConfig};
//!
//! // Use defaults from environment
//! init_logging(LogConfig::from_env())?;
//!
//! // Or configure explicitly
//! init_logging(LogConfig {
//!     format: LogFormat::Json,
//!     default_level: "info".to_string(),
//!     ..Default::default()
//! })?;
//! ```

mod config;
mod json_layer;

pub use config::{init_logging, LogConfig, LogFormat, TimestampFormat};
pub use json_layer::JsonLogEvent;
