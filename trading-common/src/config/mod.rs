//! Configuration management for the trading system.
//!
//! This module provides a hybrid configuration approach:
//! - TOML files for defaults (git-tracked, with comments)
//! - JSON for user overrides (easy CRUD from UI)
//! - Runtime merge with user overrides taking precedence
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     Configuration Flow                          │
//! │                                                                 │
//! │  TOML Files          JSON Config Store         Runtime Config   │
//! │  (git tracked)  ───► (user customizations) ───► (merged)       │
//! │                                                                 │
//! │  development.toml    user_config.json          AppConfig       │
//! │  (defaults)          (CRUD operations)         (final values)  │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```no_run
//! use trading_common::config::{ConfigService, AppConfig};
//!
//! // Load configuration from TOML defaults
//! let service = ConfigService::new("config/development.toml").unwrap();
//!
//! // Get the merged configuration
//! let config = service.get_config();
//! println!("Trading symbols: {:?}", config.symbols);
//!
//! // Update a section (stored in user overrides)
//! service.update_section("symbols", serde_json::json!(["BTCUSDT", "ETHUSDT"])).unwrap();
//!
//! // Save user overrides to disk
//! service.save().unwrap();
//! ```
//!
//! # Validation
//!
//! All configuration changes are validated before being applied:
//!
//! ```no_run
//! use trading_common::config::{ConfigService, ConfigValidator};
//!
//! let service = ConfigService::with_defaults(Default::default());
//!
//! // Validation happens automatically on update
//! let result = service.update_section("symbols", serde_json::json!(["btcusdt"])); // lowercase
//! assert!(result.is_err()); // Fails validation
//!
//! // Or validate manually
//! let validation = service.validate();
//! if !validation.valid {
//!     for error in validation.errors {
//!         println!("Error in {}: {}", error.field, error.message);
//!     }
//! }
//! ```
//!
//! # Security
//!
//! Sensitive sections are blocked from UI access:
//!
//! ```no_run
//! use trading_common::config::is_sensitive_section;
//!
//! assert!(is_sensitive_section("database.url")); // Blocked
//! assert!(is_sensitive_section("server.host")); // Blocked
//! assert!(!is_sensitive_section("symbols")); // Allowed
//! ```

pub mod schema;
pub mod service;
pub mod validation;

// Re-export main types
pub use schema::{
    is_sensitive_section, AccountsConfigSchema, AlertingConfig, AppConfig, BacktestConfig,
    BuiltinStrategiesConfig, CacheConfig, DatabaseConfig, DefaultAccountConfig, ExecutionConfig,
    FeeRate, FeesConfig, HotReloadConfig, LatencyConfig, MemoryCacheConfig, PaperTradingConfig,
    PythonStrategyConfig, PythonStrategyLimits, RedisCacheConfig, RsiStrategyConfig,
    ServiceConfig, SimulationAccountConfig, SmaStrategyConfig, StrategiesConfig, ValidationConfig,
    SENSITIVE_FIELDS,
};

pub use service::{ConfigAction, ConfigAuditEntry, ConfigError, ConfigService};

pub use validation::{
    ConfigValidator, ValidationError, ValidationErrorCode, ValidationResult, ValidationWarning,
};
