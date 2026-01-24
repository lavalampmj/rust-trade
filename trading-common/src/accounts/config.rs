//! Account configuration for loading from TOML.
//!
//! This module provides configuration structs for defining simulation accounts
//! in TOML configuration files.

use crate::accounts::Account;
use crate::orders::AccountId;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;

/// Root configuration for accounts.
#[derive(Debug, Clone, Deserialize)]
pub struct AccountsConfig {
    /// Default account used when no account_id specified on order
    pub default: DefaultAccountConfig,
    /// Additional simulation accounts
    #[serde(default)]
    pub simulation: Vec<SimulationAccountConfig>,
}

/// Configuration for the default account.
#[derive(Debug, Clone, Deserialize)]
pub struct DefaultAccountConfig {
    /// Account identifier
    pub id: String,
    /// Base currency (e.g., "USDT")
    pub currency: String,
    /// Initial balance
    pub initial_balance: Decimal,
}

/// Configuration for a simulation account.
#[derive(Debug, Clone, Deserialize)]
pub struct SimulationAccountConfig {
    /// Account identifier
    pub id: String,
    /// Base currency (e.g., "USDT")
    pub currency: String,
    /// Initial balance
    pub initial_balance: Decimal,
    /// Strategy IDs that use this account
    #[serde(default)]
    pub strategies: Vec<String>,
}

impl AccountsConfig {
    /// Get account ID for a given strategy.
    ///
    /// Searches through simulation accounts to find one that has the strategy
    /// in its `strategies` list. Falls back to the default account if not found.
    pub fn account_for_strategy(&self, strategy_id: &str) -> &str {
        for sim in &self.simulation {
            if sim.strategies.contains(&strategy_id.to_string()) {
                return &sim.id;
            }
        }
        &self.default.id
    }

    /// Build Account instances from config.
    ///
    /// Returns a vector of all accounts (default + simulation accounts).
    pub fn build_accounts(&self) -> Vec<Account> {
        let mut accounts = vec![self.default.to_account()];
        for sim in &self.simulation {
            accounts.push(sim.to_account());
        }
        accounts
    }

    /// Build a HashMap of accounts keyed by AccountId.
    pub fn build_accounts_map(&self) -> HashMap<AccountId, Account> {
        let mut map = HashMap::new();
        for account in self.build_accounts() {
            map.insert(account.id.clone(), account);
        }
        map
    }

    /// Get the default account ID.
    pub fn default_account_id(&self) -> AccountId {
        AccountId::new(&self.default.id)
    }

    /// Create an empty config with a single default account.
    pub fn empty(initial_balance: Decimal) -> Self {
        Self {
            default: DefaultAccountConfig {
                id: "DEFAULT".to_string(),
                currency: "USDT".to_string(),
                initial_balance,
            },
            simulation: Vec::new(),
        }
    }

    /// Create config with a custom default account.
    pub fn with_default(id: &str, currency: &str, initial_balance: Decimal) -> Self {
        Self {
            default: DefaultAccountConfig {
                id: id.to_string(),
                currency: currency.to_string(),
                initial_balance,
            },
            simulation: Vec::new(),
        }
    }

    /// Add a simulation account to the config (builder pattern).
    pub fn add_simulation_account(
        mut self,
        id: &str,
        currency: &str,
        initial_balance: Decimal,
        strategies: Vec<String>,
    ) -> Self {
        self.simulation.push(SimulationAccountConfig {
            id: id.to_string(),
            currency: currency.to_string(),
            initial_balance,
            strategies,
        });
        self
    }
}

impl Default for AccountsConfig {
    fn default() -> Self {
        Self::empty(Decimal::ZERO)
    }
}

impl DefaultAccountConfig {
    /// Convert to an Account instance.
    pub fn to_account(&self) -> Account {
        let mut account = Account::simulated(&self.id, &self.currency);
        account.deposit(&self.currency, self.initial_balance);
        account
    }
}

impl SimulationAccountConfig {
    /// Convert to an Account instance.
    pub fn to_account(&self) -> Account {
        let mut account = Account::simulated(&self.id, &self.currency);
        account.deposit(&self.currency, self.initial_balance);
        account
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_accounts_config_build_accounts() {
        let config = AccountsConfig {
            default: DefaultAccountConfig {
                id: "SIM-001".to_string(),
                currency: "USDT".to_string(),
                initial_balance: dec!(100000),
            },
            simulation: vec![
                SimulationAccountConfig {
                    id: "SIM-AGGRESSIVE".to_string(),
                    currency: "USDT".to_string(),
                    initial_balance: dec!(50000),
                    strategies: vec!["momentum".to_string(), "breakout".to_string()],
                },
                SimulationAccountConfig {
                    id: "SIM-CONSERVATIVE".to_string(),
                    currency: "USDT".to_string(),
                    initial_balance: dec!(200000),
                    strategies: vec!["mean_reversion".to_string()],
                },
            ],
        };

        let accounts = config.build_accounts();
        assert_eq!(accounts.len(), 3);

        // Check default account
        assert_eq!(accounts[0].id.as_str(), "SIM-001");
        assert_eq!(accounts[0].total_base(), dec!(100000));

        // Check simulation accounts
        assert_eq!(accounts[1].id.as_str(), "SIM-AGGRESSIVE");
        assert_eq!(accounts[1].total_base(), dec!(50000));

        assert_eq!(accounts[2].id.as_str(), "SIM-CONSERVATIVE");
        assert_eq!(accounts[2].total_base(), dec!(200000));
    }

    #[test]
    fn test_account_for_strategy() {
        let config = AccountsConfig {
            default: DefaultAccountConfig {
                id: "DEFAULT".to_string(),
                currency: "USDT".to_string(),
                initial_balance: dec!(100000),
            },
            simulation: vec![
                SimulationAccountConfig {
                    id: "ACCOUNT-A".to_string(),
                    currency: "USDT".to_string(),
                    initial_balance: dec!(50000),
                    strategies: vec!["momentum".to_string(), "breakout".to_string()],
                },
                SimulationAccountConfig {
                    id: "ACCOUNT-B".to_string(),
                    currency: "USDT".to_string(),
                    initial_balance: dec!(75000),
                    strategies: vec!["rsi".to_string()],
                },
            ],
        };

        // Strategy mapped to ACCOUNT-A
        assert_eq!(config.account_for_strategy("momentum"), "ACCOUNT-A");
        assert_eq!(config.account_for_strategy("breakout"), "ACCOUNT-A");

        // Strategy mapped to ACCOUNT-B
        assert_eq!(config.account_for_strategy("rsi"), "ACCOUNT-B");

        // Unknown strategy falls back to default
        assert_eq!(config.account_for_strategy("unknown"), "DEFAULT");
    }

    #[test]
    fn test_build_accounts_map() {
        let config = AccountsConfig {
            default: DefaultAccountConfig {
                id: "DEFAULT".to_string(),
                currency: "USDT".to_string(),
                initial_balance: dec!(100000),
            },
            simulation: vec![SimulationAccountConfig {
                id: "SIM-001".to_string(),
                currency: "USDT".to_string(),
                initial_balance: dec!(50000),
                strategies: vec![],
            }],
        };

        let map = config.build_accounts_map();
        assert_eq!(map.len(), 2);
        assert!(map.contains_key(&AccountId::new("DEFAULT")));
        assert!(map.contains_key(&AccountId::new("SIM-001")));
    }

    #[test]
    fn test_empty_config() {
        let config = AccountsConfig::empty(dec!(10000));

        assert_eq!(config.default.id, "DEFAULT");
        assert_eq!(config.default.initial_balance, dec!(10000));
        assert!(config.simulation.is_empty());

        let accounts = config.build_accounts();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].total_base(), dec!(10000));
    }

    #[test]
    fn test_default_impl() {
        let config = AccountsConfig::default();

        assert_eq!(config.default.id, "DEFAULT");
        assert_eq!(config.default.currency, "USDT");
        assert_eq!(config.default.initial_balance, Decimal::ZERO);
        assert!(config.simulation.is_empty());
    }

    #[test]
    fn test_builder_pattern() {
        let config = AccountsConfig::with_default("MAIN", "USDT", dec!(100000))
            .add_simulation_account(
                "AGGRESSIVE",
                "USDT",
                dec!(50000),
                vec!["momentum".to_string(), "breakout".to_string()],
            )
            .add_simulation_account(
                "CONSERVATIVE",
                "USDT",
                dec!(200000),
                vec!["mean_reversion".to_string()],
            );

        assert_eq!(config.default.id, "MAIN");
        assert_eq!(config.default.initial_balance, dec!(100000));
        assert_eq!(config.simulation.len(), 2);

        assert_eq!(config.simulation[0].id, "AGGRESSIVE");
        assert_eq!(config.simulation[0].strategies.len(), 2);

        assert_eq!(config.simulation[1].id, "CONSERVATIVE");
        assert_eq!(config.simulation[1].initial_balance, dec!(200000));

        // Test strategy lookups
        assert_eq!(config.account_for_strategy("momentum"), "AGGRESSIVE");
        assert_eq!(
            config.account_for_strategy("mean_reversion"),
            "CONSERVATIVE"
        );
        assert_eq!(config.account_for_strategy("unknown"), "MAIN");
    }

    #[test]
    fn test_toml_parsing() {
        let toml_str = r#"
[default]
id = "SIM-001"
currency = "USDT"
initial_balance = "100000"

[[simulation]]
id = "SIM-AGGRESSIVE"
currency = "USDT"
initial_balance = "50000"
strategies = ["momentum", "breakout"]

[[simulation]]
id = "SIM-CONSERVATIVE"
currency = "USDT"
initial_balance = "200000"
strategies = ["mean_reversion", "rsi"]
"#;

        let config: AccountsConfig = toml::from_str(toml_str).expect("Failed to parse TOML");

        assert_eq!(config.default.id, "SIM-001");
        assert_eq!(config.default.initial_balance, dec!(100000));

        assert_eq!(config.simulation.len(), 2);
        assert_eq!(config.simulation[0].id, "SIM-AGGRESSIVE");
        assert_eq!(config.simulation[0].strategies.len(), 2);

        assert_eq!(config.simulation[1].id, "SIM-CONSERVATIVE");
        assert_eq!(config.simulation[1].strategies.len(), 2);
    }
}
