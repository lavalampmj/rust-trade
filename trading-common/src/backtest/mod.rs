pub mod bar_generator;
pub mod engine;
pub mod metrics;
pub mod multi_engine;
pub mod portfolio;
pub mod strategy;

pub use bar_generator::{HistoricalOHLCGenerator, SessionAwareConfig};
pub use engine::{BacktestConfig, BacktestData, BacktestEngine, BacktestResult};
pub use multi_engine::{MultiStrategyBacktestEngine, StrategyRunner};
pub use portfolio::{Portfolio, Position, Trade};
pub use strategy::{create_strategy, list_strategies, Strategy, StrategyInfo};
