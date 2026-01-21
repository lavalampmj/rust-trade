//! Integration Test Harness for the Trading Data Pipeline
//!
//! This crate provides an end-to-end integration test harness for stress testing
//! the full data pipeline:
//!
//! ```text
//! Data Emulator → data-manager (WebSocket) → IPC → trading-core → strategies
//! ```
//!
//! ## Components
//!
//! - **config**: Configuration structs for test parameters (volume profiles, symbols, etc.)
//! - **generator**: Deterministic test data generator producing DBN-format ticks
//! - **emulator**: Mock LiveStreamProvider that replays generated data with timestamp rewriting
//! - **metrics**: Latency statistics collection with percentile calculation
//! - **runner**: Strategy runner framework for concurrent Rust and Python strategies
//! - **report**: Formatted report generation
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use integration_tests::{
//!     config::{IntegrationTestConfig, VolumeProfile},
//!     generator::TestDataGenerator,
//!     emulator::TestDataEmulator,
//!     runner::StrategyRunnerManager,
//!     metrics::MetricsCollector,
//!     report::{generate_report, ReportFormat},
//! };
//!
//! // Use preset configuration
//! let config = IntegrationTestConfig::normal();
//!
//! // Generate test data
//! let mut generator = TestDataGenerator::new(config.data_gen.clone());
//! let bundle = generator.generate();
//!
//! // Create emulator
//! let emulator = TestDataEmulator::new(bundle, config.emulator.clone());
//!
//! // Create strategy runners
//! let manager = StrategyRunnerManager::from_config(
//!     &config.strategies,
//!     config.metrics.latency_sample_limit,
//! );
//!
//! // Run test and collect metrics
//! // ... (see e2e_pipeline.rs for full example)
//!
//! // Generate report
//! let results = metrics_collector.build_results(/* ... */);
//! println!("{}", generate_report(&results, ReportFormat::Pretty));
//! ```
//!
//! ## Volume Profiles
//!
//! The harness supports three volume profiles:
//!
//! | Profile | Ticks/sec/symbol | Use Case |
//! |---------|------------------|----------|
//! | Lite    | 25               | Quick validation (~15 seconds) |
//! | Normal  | 250              | Standard stress test (~2 minutes) |
//! | Heavy   | 1000             | Full stress test (~5 minutes) |
//!
//! ## Test Symbols
//!
//! Generated symbols follow the pattern `TEST0000`, `TEST0001`, etc., allowing
//! easy identification of test data in logs and databases.

pub mod config;
pub mod db_verifier;
pub mod db_writer;
pub mod emulator;
pub mod generator;
pub mod metrics;
pub mod report;
pub mod runner;
pub mod strategies;
pub mod transport;

// Re-export commonly used types
pub use config::{
    DataGenConfig, EmulatorConfig, IntegrationTestConfig, MetricsConfig, StrategyConfig,
    TestConfig, VolumeProfile,
};
pub use emulator::{extract_latency_us, EmulatorMetrics, TestDataEmulator};
pub use generator::{TestDataBundle, TestDataGenerator, TestDataMetadata};
pub use metrics::{LatencyStats, MetricsCollector, StrategyMetrics, TestResults};
pub use report::{generate_report, ReportFormat};
pub use runner::{
    PythonStrategyRunner, RustStrategyRunner, StrategyRunner, StrategyRunnerFactory,
    StrategyRunnerManager,
};
pub use strategies::TestTickCounterStrategy;
pub use transport::{
    create_transport, DirectTransport, TickTransport, TransportConfig, TransportMode,
    WebSocketConfig, WebSocketTransport,
};
pub use db_verifier::{DbVerifier, DbVerifierError, DbVerifierResult, DbVerificationOptions, PersistedTickStats};
pub use db_writer::{DbWriter, DbWriterConfig, DbWriterError, DbWriterMetrics, DbWriterResult};
