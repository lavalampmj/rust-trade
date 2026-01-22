//! Account struct and balance management.
//!
//! This module provides the Account struct that tracks balances, margin,
//! and trading state separate from position management.

use super::types::{AccountState, AccountType, MarginMode, PositionMode};
use crate::orders::AccountId;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Balance information for a single currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountBalance {
    /// Currency of this balance
    pub currency: String,
    /// Total balance in the account
    pub total: Decimal,
    /// Available balance for trading (total - locked)
    pub free: Decimal,
    /// Locked balance (in open orders, margin, etc.)
    pub locked: Decimal,
}

impl AccountBalance {
    /// Create a new account balance
    pub fn new(currency: impl Into<String>, total: Decimal) -> Self {
        Self {
            currency: currency.into(),
            total,
            free: total,
            locked: Decimal::ZERO,
        }
    }

    /// Create a balance with specific free/locked amounts
    pub fn with_locked(currency: impl Into<String>, total: Decimal, locked: Decimal) -> Self {
        Self {
            currency: currency.into(),
            total,
            free: total - locked,
            locked,
        }
    }

    /// Lock funds for an order
    pub fn lock(&mut self, amount: Decimal) -> Result<(), String> {
        if amount > self.free {
            return Err(format!(
                "Insufficient free balance: need {}, available {}",
                amount, self.free
            ));
        }
        self.free -= amount;
        self.locked += amount;
        Ok(())
    }

    /// Unlock funds (e.g., order canceled)
    pub fn unlock(&mut self, amount: Decimal) -> Result<(), String> {
        if amount > self.locked {
            return Err(format!(
                "Cannot unlock more than locked: {} > {}",
                amount, self.locked
            ));
        }
        self.locked -= amount;
        self.free += amount;
        Ok(())
    }

    /// Add funds to balance
    pub fn deposit(&mut self, amount: Decimal) {
        self.total += amount;
        self.free += amount;
    }

    /// Remove funds from balance (uses free balance)
    pub fn withdraw(&mut self, amount: Decimal) -> Result<(), String> {
        if amount > self.free {
            return Err(format!(
                "Insufficient free balance for withdrawal: {} > {}",
                amount, self.free
            ));
        }
        self.free -= amount;
        self.total -= amount;
        Ok(())
    }

    /// Transfer from locked to reduce total (order filled)
    pub fn fill(&mut self, amount: Decimal) -> Result<(), String> {
        if amount > self.locked {
            return Err(format!(
                "Cannot fill more than locked: {} > {}",
                amount, self.locked
            ));
        }
        self.locked -= amount;
        self.total -= amount;
        Ok(())
    }

    /// Check if there's sufficient free balance
    pub fn has_sufficient(&self, amount: Decimal) -> bool {
        self.free >= amount
    }
}

impl Default for AccountBalance {
    fn default() -> Self {
        Self {
            currency: "USDT".to_string(),
            total: Decimal::ZERO,
            free: Decimal::ZERO,
            locked: Decimal::ZERO,
        }
    }
}

/// Margin account details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarginAccount {
    /// Total margin balance
    pub margin_balance: Decimal,
    /// Margin used by positions
    pub margin_used: Decimal,
    /// Available margin
    pub margin_available: Decimal,
    /// Initial margin requirement
    pub margin_initial: Decimal,
    /// Maintenance margin requirement
    pub margin_maintenance: Decimal,
    /// Unrealized PnL of all positions
    pub unrealized_pnl: Decimal,
    /// Maximum leverage allowed
    pub max_leverage: Decimal,
    /// Current effective leverage
    pub leverage: Decimal,
    /// Margin mode (cross or isolated)
    pub margin_mode: MarginMode,
    /// Position mode (hedge or one-way)
    pub position_mode: PositionMode,
    /// Margin call level (e.g., 0.9 = 90%)
    pub margin_call_level: Decimal,
    /// Liquidation level (e.g., 1.0 = 100% of maintenance)
    pub liquidation_level: Decimal,
}

impl MarginAccount {
    /// Create a new margin account with initial balance
    pub fn new(margin_balance: Decimal, max_leverage: Decimal) -> Self {
        Self {
            margin_balance,
            margin_used: Decimal::ZERO,
            margin_available: margin_balance,
            margin_initial: Decimal::ZERO,
            margin_maintenance: Decimal::ZERO,
            unrealized_pnl: Decimal::ZERO,
            max_leverage,
            leverage: Decimal::ONE,
            margin_mode: MarginMode::Cross,
            position_mode: PositionMode::OneWay,
            margin_call_level: Decimal::new(9, 1),  // 0.9 = 90%
            liquidation_level: Decimal::ONE,        // 1.0 = 100%
        }
    }

    /// Calculate margin ratio (used / balance)
    pub fn margin_ratio(&self) -> Decimal {
        if self.margin_balance.is_zero() {
            Decimal::ZERO
        } else {
            self.margin_used / self.margin_balance
        }
    }

    /// Calculate equity (balance + unrealized PnL)
    pub fn equity(&self) -> Decimal {
        self.margin_balance + self.unrealized_pnl
    }

    /// Check if margin call has been triggered
    pub fn is_margin_call(&self) -> bool {
        if self.margin_maintenance.is_zero() {
            return false;
        }
        let equity = self.equity();
        equity <= self.margin_maintenance * self.margin_call_level
    }

    /// Check if liquidation has been triggered
    pub fn is_liquidation(&self) -> bool {
        if self.margin_maintenance.is_zero() {
            return false;
        }
        let equity = self.equity();
        equity <= self.margin_maintenance * self.liquidation_level
    }

    /// Calculate available margin for new positions
    pub fn available_for_trading(&self) -> Decimal {
        let equity = self.equity();
        if equity > self.margin_used {
            equity - self.margin_used
        } else {
            Decimal::ZERO
        }
    }

    /// Calculate buying power (available margin * leverage)
    pub fn buying_power(&self) -> Decimal {
        self.available_for_trading() * self.max_leverage
    }

    /// Update margin used and maintenance
    pub fn update_margin(&mut self, used: Decimal, maintenance: Decimal) {
        self.margin_used = used;
        self.margin_maintenance = maintenance;
        self.margin_available = self.margin_balance - used;
        if !self.margin_used.is_zero() {
            self.leverage = self.margin_balance / self.margin_used;
        }
    }

    /// Update unrealized PnL
    pub fn update_unrealized_pnl(&mut self, pnl: Decimal) {
        self.unrealized_pnl = pnl;
    }
}

impl Default for MarginAccount {
    fn default() -> Self {
        Self::new(Decimal::ZERO, Decimal::new(1, 0))
    }
}

/// Main Account struct for tracking balances and trading state.
///
/// Separate from Portfolio to allow:
/// - Multiple accounts per venue
/// - Different account types (spot vs margin)
/// - Clean separation between balance tracking and position tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    /// Unique account identifier
    pub id: AccountId,
    /// Account type (Cash, Margin)
    pub account_type: AccountType,
    /// Current account state
    pub state: AccountState,
    /// Base/settlement currency for PnL calculation
    pub base_currency: String,
    /// Balances by currency
    pub balances: HashMap<String, AccountBalance>,
    /// Margin account details (if margin account)
    pub margin: Option<MarginAccount>,
    /// Account creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
    /// Whether this is a simulated/paper account
    pub is_simulated: bool,
}

impl Account {
    /// Create a new cash account
    pub fn cash(id: impl Into<String>, base_currency: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: AccountId::new(id),
            account_type: AccountType::Cash,
            state: AccountState::Active,
            base_currency: base_currency.into(),
            balances: HashMap::new(),
            margin: None,
            created_at: now,
            updated_at: now,
            is_simulated: false,
        }
    }

    /// Create a new margin account
    pub fn margin(
        id: impl Into<String>,
        base_currency: impl Into<String>,
        max_leverage: Decimal,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: AccountId::new(id),
            account_type: AccountType::Margin,
            state: AccountState::Active,
            base_currency: base_currency.into(),
            balances: HashMap::new(),
            margin: Some(MarginAccount::new(Decimal::ZERO, max_leverage)),
            created_at: now,
            updated_at: now,
            is_simulated: false,
        }
    }

    /// Create a simulated/paper trading account
    pub fn simulated(id: impl Into<String>, base_currency: impl Into<String>) -> Self {
        let mut account = Self::cash(id, base_currency);
        account.is_simulated = true;
        account
    }

    /// Mark this as a simulated account
    pub fn with_simulated(mut self, simulated: bool) -> Self {
        self.is_simulated = simulated;
        self
    }

    /// Get balance for a currency
    pub fn balance(&self, currency: &str) -> Option<&AccountBalance> {
        self.balances.get(currency)
    }

    /// Get mutable balance for a currency
    pub fn balance_mut(&mut self, currency: &str) -> Option<&mut AccountBalance> {
        self.updated_at = Utc::now();
        self.balances.get_mut(currency)
    }

    /// Get or create balance for a currency
    pub fn balance_or_create(&mut self, currency: &str) -> &mut AccountBalance {
        self.updated_at = Utc::now();
        self.balances
            .entry(currency.to_string())
            .or_insert_with(|| AccountBalance::new(currency, Decimal::ZERO))
    }

    /// Set balance for a currency
    pub fn set_balance(&mut self, currency: impl Into<String>, total: Decimal) {
        let currency = currency.into();
        self.balances.insert(currency.clone(), AccountBalance::new(currency, total));
        self.updated_at = Utc::now();

        // Update margin balance if this is the base currency
        if let Some(margin) = &mut self.margin {
            if self.balances.contains_key(&self.base_currency) {
                margin.margin_balance = self.balances[&self.base_currency].total;
                margin.margin_available = margin.margin_balance - margin.margin_used;
            }
        }
    }

    /// Deposit funds
    pub fn deposit(&mut self, currency: &str, amount: Decimal) {
        let balance = self.balance_or_create(currency);
        balance.deposit(amount);

        // Update margin balance if depositing base currency
        if currency == self.base_currency {
            if let Some(margin) = &mut self.margin {
                margin.margin_balance += amount;
                margin.margin_available += amount;
            }
        }
    }

    /// Withdraw funds
    pub fn withdraw(&mut self, currency: &str, amount: Decimal) -> Result<(), String> {
        let balance = self
            .balances
            .get_mut(currency)
            .ok_or_else(|| format!("No balance for currency {}", currency))?;
        balance.withdraw(amount)?;
        self.updated_at = Utc::now();

        // Update margin balance if withdrawing base currency
        if currency == self.base_currency {
            if let Some(margin) = &mut self.margin {
                margin.margin_balance -= amount;
                margin.margin_available = (margin.margin_balance - margin.margin_used).max(Decimal::ZERO);
            }
        }

        Ok(())
    }

    /// Get total account value in base currency
    /// Note: This requires price conversion for non-base currencies
    pub fn total_value(&self, prices: &HashMap<String, Decimal>) -> Decimal {
        let mut total = Decimal::ZERO;

        for (currency, balance) in &self.balances {
            if currency == &self.base_currency {
                total += balance.total;
            } else if let Some(price) = prices.get(currency) {
                total += balance.total * price;
            }
        }

        total
    }

    /// Get free balance in base currency
    pub fn free_base(&self) -> Decimal {
        self.balances
            .get(&self.base_currency)
            .map(|b| b.free)
            .unwrap_or(Decimal::ZERO)
    }

    /// Get total balance in base currency
    pub fn total_base(&self) -> Decimal {
        self.balances
            .get(&self.base_currency)
            .map(|b| b.total)
            .unwrap_or(Decimal::ZERO)
    }

    /// Check if account can trade
    pub fn can_trade(&self) -> bool {
        self.state.can_trade()
    }

    /// Check if account is in reduce-only mode
    pub fn is_reduce_only(&self) -> bool {
        self.state.reduce_only()
    }

    /// Suspend the account
    pub fn suspend(&mut self) {
        self.state = AccountState::Suspended;
        self.updated_at = Utc::now();
    }

    /// Activate the account
    pub fn activate(&mut self) {
        self.state = AccountState::Active;
        self.updated_at = Utc::now();
    }

    /// Close the account
    pub fn close(&mut self) {
        self.state = AccountState::Closed;
        self.updated_at = Utc::now();
    }

    /// Check if this is a margin account
    pub fn is_margin_account(&self) -> bool {
        self.account_type == AccountType::Margin
    }

    /// Get buying power (for margin accounts, this includes leverage)
    pub fn buying_power(&self) -> Decimal {
        match &self.margin {
            Some(margin) => margin.buying_power(),
            None => self.free_base(),
        }
    }

    /// Get equity (balance + unrealized PnL for margin accounts)
    pub fn equity(&self) -> Decimal {
        match &self.margin {
            Some(margin) => margin.equity(),
            None => self.total_base(),
        }
    }
}

/// Account event for tracking account changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccountEvent {
    /// Account was created
    Created {
        account_id: AccountId,
        timestamp: DateTime<Utc>,
    },
    /// Balance was updated
    BalanceUpdated {
        account_id: AccountId,
        currency: String,
        old_balance: AccountBalance,
        new_balance: AccountBalance,
        timestamp: DateTime<Utc>,
    },
    /// Account state changed
    StateChanged {
        account_id: AccountId,
        old_state: AccountState,
        new_state: AccountState,
        timestamp: DateTime<Utc>,
    },
    /// Margin updated
    MarginUpdated {
        account_id: AccountId,
        margin_used: Decimal,
        margin_available: Decimal,
        unrealized_pnl: Decimal,
        timestamp: DateTime<Utc>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_account_balance_operations() {
        let mut balance = AccountBalance::new("USDT", dec!(1000));

        assert_eq!(balance.total, dec!(1000));
        assert_eq!(balance.free, dec!(1000));
        assert_eq!(balance.locked, Decimal::ZERO);

        // Lock funds
        assert!(balance.lock(dec!(300)).is_ok());
        assert_eq!(balance.free, dec!(700));
        assert_eq!(balance.locked, dec!(300));

        // Cannot lock more than free
        assert!(balance.lock(dec!(800)).is_err());

        // Unlock funds
        assert!(balance.unlock(dec!(100)).is_ok());
        assert_eq!(balance.free, dec!(800));
        assert_eq!(balance.locked, dec!(200));

        // Fill from locked
        assert!(balance.fill(dec!(100)).is_ok());
        assert_eq!(balance.total, dec!(900));
        assert_eq!(balance.locked, dec!(100));
    }

    #[test]
    fn test_cash_account() {
        let mut account = Account::cash("test-001", "USDT");

        assert_eq!(account.account_type, AccountType::Cash);
        assert!(account.can_trade());
        assert!(account.margin.is_none());

        // Deposit
        account.deposit("USDT", dec!(10000));
        assert_eq!(account.total_base(), dec!(10000));
        assert_eq!(account.free_base(), dec!(10000));

        // Withdraw
        assert!(account.withdraw("USDT", dec!(3000)).is_ok());
        assert_eq!(account.total_base(), dec!(7000));
    }

    #[test]
    fn test_margin_account() {
        let mut account = Account::margin("margin-001", "USDT", dec!(20));

        assert_eq!(account.account_type, AccountType::Margin);
        assert!(account.is_margin_account());
        assert!(account.margin.is_some());

        // Deposit
        account.deposit("USDT", dec!(1000));

        // Check buying power with leverage
        let margin = account.margin.as_ref().unwrap();
        assert_eq!(margin.margin_balance, dec!(1000));
        assert_eq!(account.buying_power(), dec!(20000)); // 1000 * 20x
    }

    #[test]
    fn test_margin_calculations() {
        let mut margin = MarginAccount::new(dec!(10000), dec!(10));

        // No margin used initially
        assert_eq!(margin.equity(), dec!(10000));
        assert_eq!(margin.available_for_trading(), dec!(10000));
        assert_eq!(margin.buying_power(), dec!(100000));

        // Add some margin usage
        margin.update_margin(dec!(5000), dec!(2500));
        assert_eq!(margin.margin_used, dec!(5000));
        assert_eq!(margin.available_for_trading(), dec!(5000));

        // Add unrealized PnL
        margin.update_unrealized_pnl(dec!(1000));
        assert_eq!(margin.equity(), dec!(11000));

        // Negative PnL
        margin.update_unrealized_pnl(dec!(-500));
        assert_eq!(margin.equity(), dec!(9500));
    }

    #[test]
    fn test_margin_call_detection() {
        let mut margin = MarginAccount::new(dec!(10000), dec!(10));
        margin.update_margin(dec!(8000), dec!(8000));

        // Not in margin call yet
        assert!(!margin.is_margin_call());

        // Add loss to trigger margin call (90% of maintenance)
        margin.update_unrealized_pnl(dec!(-3000));
        // Equity = 10000 - 3000 = 7000
        // Maintenance = 8000
        // 7000 < 8000 * 0.9 = 7200, so margin call
        assert!(margin.is_margin_call());
    }

    #[test]
    fn test_account_state_transitions() {
        let mut account = Account::cash("test", "USDT");

        assert!(account.can_trade());
        assert!(!account.is_reduce_only());

        account.suspend();
        assert!(!account.can_trade());
        assert!(account.is_reduce_only());

        account.activate();
        assert!(account.can_trade());

        account.close();
        assert!(!account.can_trade());
    }

    #[test]
    fn test_simulated_account() {
        let account = Account::simulated("paper-001", "USDT");

        assert!(account.is_simulated);
        assert_eq!(account.account_type, AccountType::Cash);
    }

    #[test]
    fn test_total_value_calculation() {
        let mut account = Account::cash("test", "USDT");

        account.set_balance("USDT", dec!(1000));
        account.set_balance("BTC", dec!(0.5));
        account.set_balance("ETH", dec!(2));

        let mut prices = HashMap::new();
        prices.insert("BTC".to_string(), dec!(50000));
        prices.insert("ETH".to_string(), dec!(3000));

        let total = account.total_value(&prices);
        // 1000 USDT + 0.5 * 50000 + 2 * 3000 = 1000 + 25000 + 6000 = 32000
        assert_eq!(total, dec!(32000));
    }
}
