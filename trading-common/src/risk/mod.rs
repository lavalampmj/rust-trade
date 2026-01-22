//! Risk management and pre-trade validation.
//!
//! This module provides comprehensive risk management capabilities:
//!
//! # Trading State
//! Control trading behavior with states like Active, Halted, ReduceOnly
//!
//! # Fee Models
//! Various commission calculation models for different venues and tiers
//!
//! # Risk Engine
//! Pre-trade validation with configurable limits:
//! - Order quantity and notional limits
//! - Position size and concentration limits
//! - Daily loss and drawdown limits
//! - Order rate limiting
//!
//! # Example
//!
//! ```ignore
//! use trading_common::risk::{RiskEngine, RiskLimits, TradingState};
//! use rust_decimal_macros::dec;
//!
//! // Create risk engine with conservative limits
//! let mut engine = RiskEngine::conservative();
//!
//! // Update portfolio state
//! engine.update_equity(dec!(100000));
//! engine.update_position("BTCUSDT", dec!(1.5));
//! engine.update_price("BTCUSDT", dec!(50000));
//!
//! // Validate an order
//! let results = engine.validate_order(&order, Some(dec!(50000)));
//! if engine.is_valid(&results) {
//!     // Order passes risk checks
//!     engine.record_order();
//! } else {
//!     // Order rejected
//!     let failure = engine.first_failure(&results).unwrap();
//!     println!("Rejected: {}", failure);
//! }
//! ```

mod engine;
mod fee_model;
mod types;

// Re-export types
pub use types::{RiskCheckResult, RiskCheckType, RiskLevel, TradingState};

// Re-export fee models
pub use fee_model::{
    FeeModel, FeeTier, FixedFeeModel, HybridFeeModel, PercentageFeeModel, TieredFeeModel,
    ZeroFeeModel, infer_liquidity_side,
};

// Re-export risk engine
pub use engine::{PortfolioState, RiskEngine, RiskLimits};
