//! Account types and enums for account management.
//!
//! This module defines account classifications and states:
//! - `AccountType` - Cash or Margin account
//! - `AccountState` - Active, Suspended, Closed
//! - `MarginMode` - Cross or Isolated margin

use serde::{Deserialize, Serialize};
use std::fmt;

/// Account type classification.
///
/// Determines the trading capabilities and margin rules for an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AccountType {
    /// Cash account - no leverage, full collateral required
    Cash,
    /// Margin account - allows leverage with margin requirements
    Margin,
    /// Betting account - for prediction markets
    Betting,
}

impl AccountType {
    /// Returns true if this account type supports margin trading
    pub fn supports_margin(&self) -> bool {
        matches!(self, AccountType::Margin)
    }

    /// Returns true if this account type allows short selling
    pub fn allows_short(&self) -> bool {
        matches!(self, AccountType::Margin)
    }

    /// Returns true if this is a cash-only account
    pub fn is_cash(&self) -> bool {
        matches!(self, AccountType::Cash)
    }
}

impl Default for AccountType {
    fn default() -> Self {
        AccountType::Cash
    }
}

impl fmt::Display for AccountType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccountType::Cash => write!(f, "CASH"),
            AccountType::Margin => write!(f, "MARGIN"),
            AccountType::Betting => write!(f, "BETTING"),
        }
    }
}

/// Account state representing operational status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AccountState {
    /// Account is active and can trade
    Active,
    /// Account is suspended (e.g., due to margin call)
    Suspended,
    /// Account is in liquidation process
    Liquidating,
    /// Account is closed
    Closed,
}

impl AccountState {
    /// Returns true if the account can place new orders
    pub fn can_trade(&self) -> bool {
        matches!(self, AccountState::Active)
    }

    /// Returns true if the account can only reduce positions
    pub fn reduce_only(&self) -> bool {
        matches!(self, AccountState::Suspended | AccountState::Liquidating)
    }

    /// Returns true if the account is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(self, AccountState::Closed)
    }
}

impl Default for AccountState {
    fn default() -> Self {
        AccountState::Active
    }
}

impl fmt::Display for AccountState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccountState::Active => write!(f, "ACTIVE"),
            AccountState::Suspended => write!(f, "SUSPENDED"),
            AccountState::Liquidating => write!(f, "LIQUIDATING"),
            AccountState::Closed => write!(f, "CLOSED"),
        }
    }
}

/// Margin mode for derivatives trading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MarginMode {
    /// Cross margin - all positions share available margin
    Cross,
    /// Isolated margin - each position has dedicated margin
    Isolated,
}

impl MarginMode {
    /// Returns true if this is cross margin mode
    pub fn is_cross(&self) -> bool {
        matches!(self, MarginMode::Cross)
    }

    /// Returns true if this is isolated margin mode
    pub fn is_isolated(&self) -> bool {
        matches!(self, MarginMode::Isolated)
    }
}

impl Default for MarginMode {
    fn default() -> Self {
        MarginMode::Cross
    }
}

impl fmt::Display for MarginMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MarginMode::Cross => write!(f, "CROSS"),
            MarginMode::Isolated => write!(f, "ISOLATED"),
        }
    }
}

/// Position mode for derivatives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PositionMode {
    /// Hedge mode - can hold both long and short positions simultaneously
    Hedge,
    /// One-way mode - only one position direction allowed
    OneWay,
}

impl Default for PositionMode {
    fn default() -> Self {
        PositionMode::OneWay
    }
}

impl fmt::Display for PositionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PositionMode::Hedge => write!(f, "HEDGE"),
            PositionMode::OneWay => write!(f, "ONE_WAY"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_type_properties() {
        assert!(!AccountType::Cash.supports_margin());
        assert!(AccountType::Margin.supports_margin());
        assert!(!AccountType::Cash.allows_short());
        assert!(AccountType::Margin.allows_short());
    }

    #[test]
    fn test_account_state_properties() {
        assert!(AccountState::Active.can_trade());
        assert!(!AccountState::Suspended.can_trade());
        assert!(AccountState::Suspended.reduce_only());
        assert!(AccountState::Closed.is_terminal());
    }

    #[test]
    fn test_margin_mode() {
        assert!(MarginMode::Cross.is_cross());
        assert!(MarginMode::Isolated.is_isolated());
    }

    #[test]
    fn test_defaults() {
        assert_eq!(AccountType::default(), AccountType::Cash);
        assert_eq!(AccountState::default(), AccountState::Active);
        assert_eq!(MarginMode::default(), MarginMode::Cross);
        assert_eq!(PositionMode::default(), PositionMode::OneWay);
    }
}
