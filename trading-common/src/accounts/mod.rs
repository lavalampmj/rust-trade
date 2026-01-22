//! Account management and balance tracking.
//!
//! This module provides account abstractions separate from position tracking:
//!
//! # Account Types
//! - **Cash**: Full collateral required, no leverage
//! - **Margin**: Allows leverage with margin requirements
//!
//! # Features
//! - Multi-currency balance tracking
//! - Free/locked balance management
//! - Margin calculations (initial, maintenance, liquidation)
//! - Account state management (active, suspended, closed)
//!
//! # Example
//!
//! ```ignore
//! use trading_common::accounts::{Account, AccountType, MarginMode};
//! use rust_decimal_macros::dec;
//!
//! // Create a margin account
//! let mut account = Account::margin("acc-001", "USDT", dec!(20)); // 20x max leverage
//!
//! // Deposit funds
//! account.deposit("USDT", dec!(10000));
//!
//! // Check buying power
//! println!("Buying power: ${}", account.buying_power()); // 200,000
//!
//! // Get margin details
//! if let Some(margin) = &account.margin {
//!     println!("Equity: ${}", margin.equity());
//!     println!("Available: ${}", margin.available_for_trading());
//! }
//! ```

mod account;
mod types;

// Re-export types
pub use types::{AccountState, AccountType, MarginMode, PositionMode};

// Re-export account structs
pub use account::{Account, AccountBalance, AccountEvent, MarginAccount};
