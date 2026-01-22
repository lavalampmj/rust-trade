//! Risk management types and enums.
//!
//! This module defines trading states and risk-related enumerations
//! for controlling trading behavior and risk parameters.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Trading state controlling what actions are allowed.
///
/// Allows granular control over trading operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TradingState {
    /// Normal trading - all operations allowed
    Active,
    /// Trading halted - no new orders, existing orders may be canceled
    Halted,
    /// Reduce only - only orders that reduce position size allowed
    ReduceOnly,
    /// Liquidating - system is closing positions
    Liquidating,
    /// Paused - temporary pause, orders queued but not submitted
    Paused,
}

impl TradingState {
    /// Check if new orders can be placed
    pub fn can_place_orders(&self) -> bool {
        matches!(self, TradingState::Active | TradingState::ReduceOnly)
    }

    /// Check if orders can increase position
    pub fn can_increase_position(&self) -> bool {
        matches!(self, TradingState::Active)
    }

    /// Check if orders can decrease position
    pub fn can_decrease_position(&self) -> bool {
        matches!(
            self,
            TradingState::Active | TradingState::ReduceOnly | TradingState::Liquidating
        )
    }

    /// Check if cancellations are allowed
    pub fn can_cancel_orders(&self) -> bool {
        !matches!(self, TradingState::Liquidating)
    }

    /// Check if state is terminal (requires manual intervention)
    pub fn requires_intervention(&self) -> bool {
        matches!(self, TradingState::Halted | TradingState::Liquidating)
    }
}

impl Default for TradingState {
    fn default() -> Self {
        TradingState::Active
    }
}

impl fmt::Display for TradingState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TradingState::Active => write!(f, "ACTIVE"),
            TradingState::Halted => write!(f, "HALTED"),
            TradingState::ReduceOnly => write!(f, "REDUCE_ONLY"),
            TradingState::Liquidating => write!(f, "LIQUIDATING"),
            TradingState::Paused => write!(f, "PAUSED"),
        }
    }
}

/// Risk level classification for alerts and actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskLevel {
    /// No risk concerns
    Normal,
    /// Elevated risk - monitor closely
    Elevated,
    /// Warning level - consider reducing exposure
    Warning,
    /// Critical level - immediate action required
    Critical,
    /// Breach - limits exceeded
    Breach,
}

impl RiskLevel {
    /// Check if this level requires attention
    pub fn requires_attention(&self) -> bool {
        *self >= RiskLevel::Warning
    }

    /// Check if this level requires immediate action
    pub fn requires_action(&self) -> bool {
        *self >= RiskLevel::Critical
    }
}

impl Default for RiskLevel {
    fn default() -> Self {
        RiskLevel::Normal
    }
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RiskLevel::Normal => write!(f, "NORMAL"),
            RiskLevel::Elevated => write!(f, "ELEVATED"),
            RiskLevel::Warning => write!(f, "WARNING"),
            RiskLevel::Critical => write!(f, "CRITICAL"),
            RiskLevel::Breach => write!(f, "BREACH"),
        }
    }
}

/// Type of risk check that was performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskCheckType {
    /// Order quantity check
    OrderQuantity,
    /// Order notional value check
    OrderNotional,
    /// Position size check
    PositionSize,
    /// Position notional check
    PositionNotional,
    /// Account equity check
    AccountEquity,
    /// Margin check
    MarginRequirement,
    /// Drawdown check
    Drawdown,
    /// Daily loss check
    DailyLoss,
    /// Order rate check
    OrderRate,
    /// Concentration check (single symbol exposure)
    Concentration,
    /// Trading state check
    TradingState,
    /// Instrument validation
    InstrumentValidation,
    /// Price validation
    PriceValidation,
    /// Custom check
    Custom,
}

impl fmt::Display for RiskCheckType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RiskCheckType::OrderQuantity => write!(f, "ORDER_QUANTITY"),
            RiskCheckType::OrderNotional => write!(f, "ORDER_NOTIONAL"),
            RiskCheckType::PositionSize => write!(f, "POSITION_SIZE"),
            RiskCheckType::PositionNotional => write!(f, "POSITION_NOTIONAL"),
            RiskCheckType::AccountEquity => write!(f, "ACCOUNT_EQUITY"),
            RiskCheckType::MarginRequirement => write!(f, "MARGIN_REQUIREMENT"),
            RiskCheckType::Drawdown => write!(f, "DRAWDOWN"),
            RiskCheckType::DailyLoss => write!(f, "DAILY_LOSS"),
            RiskCheckType::OrderRate => write!(f, "ORDER_RATE"),
            RiskCheckType::Concentration => write!(f, "CONCENTRATION"),
            RiskCheckType::TradingState => write!(f, "TRADING_STATE"),
            RiskCheckType::InstrumentValidation => write!(f, "INSTRUMENT_VALIDATION"),
            RiskCheckType::PriceValidation => write!(f, "PRICE_VALIDATION"),
            RiskCheckType::Custom => write!(f, "CUSTOM"),
        }
    }
}

/// Result of a risk check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskCheckResult {
    /// Type of check performed
    pub check_type: RiskCheckType,
    /// Whether the check passed
    pub passed: bool,
    /// Risk level if check passed with warnings
    pub risk_level: RiskLevel,
    /// Message describing the result
    pub message: String,
    /// Current value that was checked
    pub current_value: Option<String>,
    /// Limit that was checked against
    pub limit_value: Option<String>,
}

impl RiskCheckResult {
    /// Create a passing result
    pub fn pass(check_type: RiskCheckType) -> Self {
        Self {
            check_type,
            passed: true,
            risk_level: RiskLevel::Normal,
            message: "Check passed".to_string(),
            current_value: None,
            limit_value: None,
        }
    }

    /// Create a passing result with a warning
    pub fn pass_with_warning(check_type: RiskCheckType, message: impl Into<String>) -> Self {
        Self {
            check_type,
            passed: true,
            risk_level: RiskLevel::Warning,
            message: message.into(),
            current_value: None,
            limit_value: None,
        }
    }

    /// Create a failing result
    pub fn fail(check_type: RiskCheckType, message: impl Into<String>) -> Self {
        Self {
            check_type,
            passed: false,
            risk_level: RiskLevel::Breach,
            message: message.into(),
            current_value: None,
            limit_value: None,
        }
    }

    /// Add current value to the result
    pub fn with_current(mut self, value: impl ToString) -> Self {
        self.current_value = Some(value.to_string());
        self
    }

    /// Add limit value to the result
    pub fn with_limit(mut self, value: impl ToString) -> Self {
        self.limit_value = Some(value.to_string());
        self
    }

    /// Set risk level
    pub fn with_risk_level(mut self, level: RiskLevel) -> Self {
        self.risk_level = level;
        self
    }
}

impl fmt::Display for RiskCheckResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.passed { "PASS" } else { "FAIL" };
        write!(f, "[{}] {}: {}", status, self.check_type, self.message)?;
        if let (Some(current), Some(limit)) = (&self.current_value, &self.limit_value) {
            write!(f, " (current: {}, limit: {})", current, limit)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trading_state_permissions() {
        assert!(TradingState::Active.can_place_orders());
        assert!(TradingState::Active.can_increase_position());
        assert!(TradingState::Active.can_decrease_position());

        assert!(TradingState::ReduceOnly.can_place_orders());
        assert!(!TradingState::ReduceOnly.can_increase_position());
        assert!(TradingState::ReduceOnly.can_decrease_position());

        assert!(!TradingState::Halted.can_place_orders());
        assert!(TradingState::Halted.requires_intervention());

        assert!(TradingState::Liquidating.can_decrease_position());
        assert!(!TradingState::Liquidating.can_cancel_orders());
    }

    #[test]
    fn test_risk_level_ordering() {
        assert!(RiskLevel::Normal < RiskLevel::Elevated);
        assert!(RiskLevel::Elevated < RiskLevel::Warning);
        assert!(RiskLevel::Warning < RiskLevel::Critical);
        assert!(RiskLevel::Critical < RiskLevel::Breach);

        assert!(!RiskLevel::Normal.requires_attention());
        assert!(RiskLevel::Warning.requires_attention());
        assert!(RiskLevel::Critical.requires_action());
    }

    #[test]
    fn test_risk_check_result() {
        let pass = RiskCheckResult::pass(RiskCheckType::OrderQuantity);
        assert!(pass.passed);
        assert_eq!(pass.risk_level, RiskLevel::Normal);

        let fail = RiskCheckResult::fail(RiskCheckType::PositionSize, "Position too large")
            .with_current("100")
            .with_limit("50");
        assert!(!fail.passed);
        assert_eq!(fail.current_value, Some("100".to_string()));
        assert_eq!(fail.limit_value, Some("50".to_string()));
    }

    #[test]
    fn test_defaults() {
        assert_eq!(TradingState::default(), TradingState::Active);
        assert_eq!(RiskLevel::default(), RiskLevel::Normal);
    }
}
