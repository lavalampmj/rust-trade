//! Configuration validation rules.
//!
//! This module provides validation for configuration values before they are applied.
//! Validation ensures data integrity and prevents invalid configurations from being saved.

use crate::config::schema::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Result of configuration validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Whether the configuration is valid
    pub valid: bool,
    /// List of validation errors
    pub errors: Vec<ValidationError>,
    /// List of validation warnings (non-fatal)
    pub warnings: Vec<ValidationWarning>,
}

impl ValidationResult {
    /// Create a successful validation result.
    pub fn success() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Create a failed validation result with errors.
    pub fn failure(errors: Vec<ValidationError>) -> Self {
        Self {
            valid: false,
            errors,
            warnings: Vec::new(),
        }
    }

    /// Add an error to the result.
    pub fn add_error(&mut self, error: ValidationError) {
        self.valid = false;
        self.errors.push(error);
    }

    /// Add a warning to the result.
    pub fn add_warning(&mut self, warning: ValidationWarning) {
        self.warnings.push(warning);
    }

    /// Merge another validation result into this one.
    pub fn merge(&mut self, other: ValidationResult) {
        if !other.valid {
            self.valid = false;
        }
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
    }
}

/// A validation error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    /// Field path that caused the error
    pub field: String,
    /// Error message
    pub message: String,
    /// Error code for programmatic handling
    pub code: ValidationErrorCode,
}

impl ValidationError {
    pub fn new(field: impl Into<String>, message: impl Into<String>, code: ValidationErrorCode) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            code,
        }
    }
}

/// Validation error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ValidationErrorCode {
    /// Field is required but missing
    Required,
    /// Value is out of valid range
    OutOfRange,
    /// Invalid format (e.g., not a valid decimal)
    InvalidFormat,
    /// Value is too short
    TooShort,
    /// Value is too long
    TooLong,
    /// Invalid pattern (e.g., symbol format)
    InvalidPattern,
    /// Duplicate value where uniqueness required
    Duplicate,
    /// Reference to non-existent entity
    InvalidReference,
    /// Generic validation failure
    Invalid,
}

/// A validation warning (non-fatal).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationWarning {
    /// Field path
    pub field: String,
    /// Warning message
    pub message: String,
}

impl ValidationWarning {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

/// Configuration validator.
pub struct ConfigValidator;

impl ConfigValidator {
    /// Validate the entire application configuration.
    pub fn validate(config: &AppConfig) -> ValidationResult {
        let mut result = ValidationResult::success();

        // Validate symbols
        result.merge(Self::validate_symbols(&config.symbols));

        // Validate accounts
        result.merge(Self::validate_accounts(&config.accounts));

        // Validate paper trading
        result.merge(Self::validate_paper_trading(&config.paper_trading));

        // Validate strategies
        result.merge(Self::validate_builtin_strategies(&config.builtin_strategies));
        result.merge(Self::validate_python_strategies(&config.strategies));

        // Validate execution
        result.merge(Self::validate_execution(&config.execution));

        // Validate alerting
        result.merge(Self::validate_alerting(&config.alerting));

        result
    }

    /// Validate a specific section.
    pub fn validate_section(section: &str, value: &serde_json::Value) -> ValidationResult {
        match section {
            "symbols" => {
                if let Ok(symbols) = serde_json::from_value::<Vec<String>>(value.clone()) {
                    Self::validate_symbols(&symbols)
                } else {
                    ValidationResult::failure(vec![ValidationError::new(
                        section,
                        "Invalid symbols format",
                        ValidationErrorCode::InvalidFormat,
                    )])
                }
            }
            "accounts" | "accounts.default" | "accounts.simulation" => {
                if let Ok(accounts) = serde_json::from_value::<AccountsConfigSchema>(value.clone())
                {
                    Self::validate_accounts(&accounts)
                } else {
                    ValidationResult::failure(vec![ValidationError::new(
                        section,
                        "Invalid accounts format",
                        ValidationErrorCode::InvalidFormat,
                    )])
                }
            }
            "paper_trading" => {
                if let Ok(pt) = serde_json::from_value::<PaperTradingConfig>(value.clone()) {
                    Self::validate_paper_trading(&pt)
                } else {
                    ValidationResult::failure(vec![ValidationError::new(
                        section,
                        "Invalid paper_trading format",
                        ValidationErrorCode::InvalidFormat,
                    )])
                }
            }
            "builtin_strategies" | "builtin_strategies.rsi" | "builtin_strategies.sma" => {
                if let Ok(strategies) =
                    serde_json::from_value::<BuiltinStrategiesConfig>(value.clone())
                {
                    Self::validate_builtin_strategies(&strategies)
                } else {
                    ValidationResult::failure(vec![ValidationError::new(
                        section,
                        "Invalid builtin_strategies format",
                        ValidationErrorCode::InvalidFormat,
                    )])
                }
            }
            "execution" | "execution.fees" | "execution.latency" => {
                if let Ok(execution) = serde_json::from_value::<ExecutionConfig>(value.clone()) {
                    Self::validate_execution(&execution)
                } else {
                    ValidationResult::failure(vec![ValidationError::new(
                        section,
                        "Invalid execution format",
                        ValidationErrorCode::InvalidFormat,
                    )])
                }
            }
            _ => ValidationResult::success(), // Unknown sections pass through
        }
    }

    /// Validate trading symbols.
    pub fn validate_symbols(symbols: &[String]) -> ValidationResult {
        let mut result = ValidationResult::success();

        for (i, symbol) in symbols.iter().enumerate() {
            let field = format!("symbols[{}]", i);

            // Check length
            if symbol.len() < 3 {
                result.add_error(ValidationError::new(
                    &field,
                    "Symbol must be at least 3 characters",
                    ValidationErrorCode::TooShort,
                ));
            }
            if symbol.len() > 20 {
                result.add_error(ValidationError::new(
                    &field,
                    "Symbol must be at most 20 characters",
                    ValidationErrorCode::TooLong,
                ));
            }

            // Check pattern (uppercase alphanumeric)
            if !symbol.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
                result.add_error(ValidationError::new(
                    &field,
                    "Symbol must be uppercase alphanumeric",
                    ValidationErrorCode::InvalidPattern,
                ));
            }
        }

        // Check for duplicates
        let mut seen = std::collections::HashSet::new();
        for (i, symbol) in symbols.iter().enumerate() {
            if !seen.insert(symbol) {
                result.add_error(ValidationError::new(
                    format!("symbols[{}]", i),
                    format!("Duplicate symbol: {}", symbol),
                    ValidationErrorCode::Duplicate,
                ));
            }
        }

        result
    }

    /// Validate account configuration.
    pub fn validate_accounts(accounts: &AccountsConfigSchema) -> ValidationResult {
        let mut result = ValidationResult::success();

        // Validate default account
        result.merge(Self::validate_account_id(
            "accounts.default.id",
            &accounts.default.id,
        ));
        result.merge(Self::validate_decimal(
            "accounts.default.initial_balance",
            &accounts.default.initial_balance,
            Some(Decimal::ZERO),
            None,
        ));

        // Validate simulation accounts
        let mut account_ids = std::collections::HashSet::new();
        account_ids.insert(&accounts.default.id);

        for (i, sim) in accounts.simulation.iter().enumerate() {
            let prefix = format!("accounts.simulation[{}]", i);

            result.merge(Self::validate_account_id(&format!("{}.id", prefix), &sim.id));

            if !account_ids.insert(&sim.id) {
                result.add_error(ValidationError::new(
                    format!("{}.id", prefix),
                    format!("Duplicate account ID: {}", sim.id),
                    ValidationErrorCode::Duplicate,
                ));
            }

            result.merge(Self::validate_decimal(
                &format!("{}.initial_balance", prefix),
                &sim.initial_balance,
                Some(Decimal::ZERO),
                None,
            ));
        }

        result
    }

    /// Validate an account ID.
    fn validate_account_id(field: &str, id: &str) -> ValidationResult {
        let mut result = ValidationResult::success();

        if id.is_empty() {
            result.add_error(ValidationError::new(
                field,
                "Account ID cannot be empty",
                ValidationErrorCode::Required,
            ));
        } else if id.len() > 50 {
            result.add_error(ValidationError::new(
                field,
                "Account ID must be at most 50 characters",
                ValidationErrorCode::TooLong,
            ));
        } else if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            result.add_error(ValidationError::new(
                field,
                "Account ID must be alphanumeric with hyphens/underscores only",
                ValidationErrorCode::InvalidPattern,
            ));
        }

        result
    }

    /// Validate paper trading configuration.
    pub fn validate_paper_trading(config: &PaperTradingConfig) -> ValidationResult {
        let mut result = ValidationResult::success();

        if config.initial_capital <= 0.0 {
            result.add_error(ValidationError::new(
                "paper_trading.initial_capital",
                "Initial capital must be positive",
                ValidationErrorCode::OutOfRange,
            ));
        }

        if config.strategy.is_empty() {
            result.add_error(ValidationError::new(
                "paper_trading.strategy",
                "Strategy cannot be empty",
                ValidationErrorCode::Required,
            ));
        }

        result
    }

    /// Validate built-in strategy configuration.
    pub fn validate_builtin_strategies(config: &BuiltinStrategiesConfig) -> ValidationResult {
        let mut result = ValidationResult::success();

        // RSI validation
        if config.rsi.period == 0 {
            result.add_error(ValidationError::new(
                "builtin_strategies.rsi.period",
                "RSI period must be greater than 0",
                ValidationErrorCode::OutOfRange,
            ));
        }
        if config.rsi.period > 200 {
            result.add_warning(ValidationWarning::new(
                "builtin_strategies.rsi.period",
                "RSI period over 200 may produce unreliable signals",
            ));
        }
        if config.rsi.oversold >= config.rsi.overbought {
            result.add_error(ValidationError::new(
                "builtin_strategies.rsi.oversold",
                "Oversold must be less than overbought",
                ValidationErrorCode::OutOfRange,
            ));
        }
        if config.rsi.overbought > 100 {
            result.add_error(ValidationError::new(
                "builtin_strategies.rsi.overbought",
                "Overbought must be <= 100",
                ValidationErrorCode::OutOfRange,
            ));
        }

        // SMA validation
        if config.sma.short_period == 0 {
            result.add_error(ValidationError::new(
                "builtin_strategies.sma.short_period",
                "Short period must be greater than 0",
                ValidationErrorCode::OutOfRange,
            ));
        }
        if config.sma.long_period == 0 {
            result.add_error(ValidationError::new(
                "builtin_strategies.sma.long_period",
                "Long period must be greater than 0",
                ValidationErrorCode::OutOfRange,
            ));
        }
        if config.sma.short_period >= config.sma.long_period {
            result.add_error(ValidationError::new(
                "builtin_strategies.sma.short_period",
                "Short period must be less than long period",
                ValidationErrorCode::OutOfRange,
            ));
        }

        result
    }

    /// Validate Python strategies configuration.
    pub fn validate_python_strategies(config: &StrategiesConfig) -> ValidationResult {
        let mut result = ValidationResult::success();

        let mut strategy_ids = std::collections::HashSet::new();

        for (i, strategy) in config.python.iter().enumerate() {
            let prefix = format!("strategies.python[{}]", i);

            // Validate ID
            if strategy.id.is_empty() {
                result.add_error(ValidationError::new(
                    format!("{}.id", prefix),
                    "Strategy ID cannot be empty",
                    ValidationErrorCode::Required,
                ));
            } else if !strategy
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                result.add_error(ValidationError::new(
                    format!("{}.id", prefix),
                    "Strategy ID must be lowercase alphanumeric with underscores",
                    ValidationErrorCode::InvalidPattern,
                ));
            }

            if !strategy_ids.insert(&strategy.id) {
                result.add_error(ValidationError::new(
                    format!("{}.id", prefix),
                    format!("Duplicate strategy ID: {}", strategy.id),
                    ValidationErrorCode::Duplicate,
                ));
            }

            // Validate file path
            if strategy.file.is_empty() {
                result.add_error(ValidationError::new(
                    format!("{}.file", prefix),
                    "Strategy file cannot be empty",
                    ValidationErrorCode::Required,
                ));
            } else if !strategy.file.ends_with(".py") {
                result.add_error(ValidationError::new(
                    format!("{}.file", prefix),
                    "Strategy file must be a .py file",
                    ValidationErrorCode::InvalidFormat,
                ));
            }

            // Validate class name
            if strategy.class_name.is_empty() {
                result.add_error(ValidationError::new(
                    format!("{}.class_name", prefix),
                    "Class name cannot be empty",
                    ValidationErrorCode::Required,
                ));
            } else if !strategy.class_name.chars().next().unwrap().is_ascii_uppercase() {
                result.add_warning(ValidationWarning::new(
                    format!("{}.class_name", prefix),
                    "Python class names should start with uppercase",
                ));
            }

            // Validate execution limits
            if strategy.limits.max_execution_time_ms == 0 {
                result.add_error(ValidationError::new(
                    format!("{}.limits.max_execution_time_ms", prefix),
                    "Max execution time must be greater than 0",
                    ValidationErrorCode::OutOfRange,
                ));
            }
        }

        result
    }

    /// Validate execution configuration.
    pub fn validate_execution(config: &ExecutionConfig) -> ValidationResult {
        let mut result = ValidationResult::success();

        // Validate fee model
        let valid_models = ["zero", "binance_spot", "binance_spot_bnb", "binance_futures", "custom"];
        if !valid_models.contains(&config.fees.default_model.as_str()) {
            result.add_error(ValidationError::new(
                "execution.fees.default_model",
                format!(
                    "Invalid fee model. Must be one of: {}",
                    valid_models.join(", ")
                ),
                ValidationErrorCode::Invalid,
            ));
        }

        // Validate fee rates
        result.merge(Self::validate_fee_rate(
            "execution.fees.binance_spot",
            &config.fees.binance_spot,
        ));
        result.merge(Self::validate_fee_rate(
            "execution.fees.binance_futures",
            &config.fees.binance_futures,
        ));
        result.merge(Self::validate_fee_rate(
            "execution.fees.custom",
            &config.fees.custom,
        ));

        // Validate latency model
        let valid_latency_models = ["none", "fixed", "variable"];
        if !valid_latency_models.contains(&config.latency.model.as_str()) {
            result.add_error(ValidationError::new(
                "execution.latency.model",
                format!(
                    "Invalid latency model. Must be one of: {}",
                    valid_latency_models.join(", ")
                ),
                ValidationErrorCode::Invalid,
            ));
        }

        result
    }

    /// Validate a fee rate.
    fn validate_fee_rate(field: &str, rate: &FeeRate) -> ValidationResult {
        let mut result = ValidationResult::success();

        result.merge(Self::validate_decimal(
            &format!("{}.maker", field),
            &rate.maker,
            Some(Decimal::ZERO),
            Some(Decimal::ONE),
        ));
        result.merge(Self::validate_decimal(
            &format!("{}.taker", field),
            &rate.taker,
            Some(Decimal::ZERO),
            Some(Decimal::ONE),
        ));

        result
    }

    /// Validate alerting configuration.
    pub fn validate_alerting(config: &AlertingConfig) -> ValidationResult {
        let mut result = ValidationResult::success();

        if config.interval_secs == 0 {
            result.add_error(ValidationError::new(
                "alerting.interval_secs",
                "Interval must be greater than 0",
                ValidationErrorCode::OutOfRange,
            ));
        }

        if config.pool_saturation_threshold < 0.0 || config.pool_saturation_threshold > 1.0 {
            result.add_error(ValidationError::new(
                "alerting.pool_saturation_threshold",
                "Threshold must be between 0.0 and 1.0",
                ValidationErrorCode::OutOfRange,
            ));
        }

        if config.pool_critical_threshold < 0.0 || config.pool_critical_threshold > 1.0 {
            result.add_error(ValidationError::new(
                "alerting.pool_critical_threshold",
                "Threshold must be between 0.0 and 1.0",
                ValidationErrorCode::OutOfRange,
            ));
        }

        if config.pool_saturation_threshold >= config.pool_critical_threshold {
            result.add_error(ValidationError::new(
                "alerting.pool_saturation_threshold",
                "Saturation threshold must be less than critical threshold",
                ValidationErrorCode::OutOfRange,
            ));
        }

        result
    }

    /// Validate a decimal string value.
    fn validate_decimal(
        field: &str,
        value: &str,
        min: Option<Decimal>,
        max: Option<Decimal>,
    ) -> ValidationResult {
        let mut result = ValidationResult::success();

        match Decimal::from_str(value) {
            Ok(decimal) => {
                if let Some(min_val) = min {
                    if decimal < min_val {
                        result.add_error(ValidationError::new(
                            field,
                            format!("Value must be at least {}", min_val),
                            ValidationErrorCode::OutOfRange,
                        ));
                    }
                }
                if let Some(max_val) = max {
                    if decimal > max_val {
                        result.add_error(ValidationError::new(
                            field,
                            format!("Value must be at most {}", max_val),
                            ValidationErrorCode::OutOfRange,
                        ));
                    }
                }
            }
            Err(_) => {
                result.add_error(ValidationError::new(
                    field,
                    "Invalid decimal format",
                    ValidationErrorCode::InvalidFormat,
                ));
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_result_success() {
        let result = ValidationResult::success();
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_validation_result_failure() {
        let result = ValidationResult::failure(vec![ValidationError::new(
            "test",
            "Error message",
            ValidationErrorCode::Invalid,
        )]);
        assert!(!result.valid);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_validation_result_merge() {
        let mut result1 = ValidationResult::success();
        let result2 = ValidationResult::failure(vec![ValidationError::new(
            "test",
            "Error",
            ValidationErrorCode::Invalid,
        )]);

        result1.merge(result2);
        assert!(!result1.valid);
        assert_eq!(result1.errors.len(), 1);
    }

    #[test]
    fn test_validate_symbols_valid() {
        let symbols = vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()];
        let result = ConfigValidator::validate_symbols(&symbols);
        assert!(result.valid);
    }

    #[test]
    fn test_validate_symbols_too_short() {
        let symbols = vec!["BT".to_string()];
        let result = ConfigValidator::validate_symbols(&symbols);
        assert!(!result.valid);
        assert!(result.errors[0].message.contains("at least 3"));
    }

    #[test]
    fn test_validate_symbols_too_long() {
        let symbols = vec!["ABCDEFGHIJKLMNOPQRSTUVWXYZ".to_string()];
        let result = ConfigValidator::validate_symbols(&symbols);
        assert!(!result.valid);
        assert!(result.errors[0].message.contains("at most 20"));
    }

    #[test]
    fn test_validate_symbols_invalid_pattern() {
        let symbols = vec!["btcusdt".to_string()]; // lowercase
        let result = ConfigValidator::validate_symbols(&symbols);
        assert!(!result.valid);
        assert!(result.errors[0].message.contains("uppercase"));
    }

    #[test]
    fn test_validate_symbols_duplicate() {
        let symbols = vec!["BTCUSDT".to_string(), "BTCUSDT".to_string()];
        let result = ConfigValidator::validate_symbols(&symbols);
        assert!(!result.valid);
        assert!(result.errors[0].message.contains("Duplicate"));
    }

    #[test]
    fn test_validate_accounts_valid() {
        let accounts = AccountsConfigSchema::default();
        let result = ConfigValidator::validate_accounts(&accounts);
        assert!(result.valid);
    }

    #[test]
    fn test_validate_accounts_duplicate_id() {
        let accounts = AccountsConfigSchema {
            default: DefaultAccountConfig {
                id: "MAIN".to_string(),
                currency: "USDT".to_string(),
                initial_balance: "10000".to_string(),
            },
            simulation: vec![SimulationAccountConfig {
                id: "MAIN".to_string(), // Duplicate
                currency: "USDT".to_string(),
                initial_balance: "5000".to_string(),
                strategies: vec![],
            }],
        };
        let result = ConfigValidator::validate_accounts(&accounts);
        assert!(!result.valid);
        assert!(result.errors[0].message.contains("Duplicate"));
    }

    #[test]
    fn test_validate_accounts_invalid_balance() {
        let accounts = AccountsConfigSchema {
            default: DefaultAccountConfig {
                id: "MAIN".to_string(),
                currency: "USDT".to_string(),
                initial_balance: "not_a_number".to_string(),
            },
            simulation: vec![],
        };
        let result = ConfigValidator::validate_accounts(&accounts);
        assert!(!result.valid);
        assert!(result.errors[0].message.contains("Invalid decimal"));
    }

    #[test]
    fn test_validate_paper_trading_valid() {
        let config = PaperTradingConfig::default();
        let result = ConfigValidator::validate_paper_trading(&config);
        assert!(result.valid);
    }

    #[test]
    fn test_validate_paper_trading_negative_capital() {
        let config = PaperTradingConfig {
            enabled: true,
            strategy: "rsi".to_string(),
            initial_capital: -1000.0,
        };
        let result = ConfigValidator::validate_paper_trading(&config);
        assert!(!result.valid);
        assert!(result.errors[0].message.contains("positive"));
    }

    #[test]
    fn test_validate_rsi_strategy_valid() {
        let config = BuiltinStrategiesConfig::default();
        let result = ConfigValidator::validate_builtin_strategies(&config);
        assert!(result.valid);
    }

    #[test]
    fn test_validate_rsi_strategy_invalid_thresholds() {
        let config = BuiltinStrategiesConfig {
            rsi: RsiStrategyConfig {
                period: 14,
                oversold: 70, // Greater than overbought
                overbought: 30,
                ..Default::default()
            },
            sma: SmaStrategyConfig::default(),
        };
        let result = ConfigValidator::validate_builtin_strategies(&config);
        assert!(!result.valid);
    }

    #[test]
    fn test_validate_sma_strategy_invalid_periods() {
        let config = BuiltinStrategiesConfig {
            rsi: RsiStrategyConfig::default(),
            sma: SmaStrategyConfig {
                short_period: 20,  // Greater than long
                long_period: 5,
                ..Default::default()
            },
        };
        let result = ConfigValidator::validate_builtin_strategies(&config);
        assert!(!result.valid);
    }

    #[test]
    fn test_validate_python_strategy_valid() {
        let config = StrategiesConfig {
            python_dir: "strategies".to_string(),
            hot_reload: false,
            hot_reload_config: HotReloadConfig::default(),
            python: vec![PythonStrategyConfig {
                id: "my_strategy".to_string(),
                file: "my_strategy.py".to_string(),
                class_name: "MyStrategy".to_string(),
                description: "Test".to_string(),
                enabled: true,
                sha256: "abc".to_string(),
                limits: PythonStrategyLimits::default(),
            }],
        };
        let result = ConfigValidator::validate_python_strategies(&config);
        assert!(result.valid);
    }

    #[test]
    fn test_validate_python_strategy_invalid_id() {
        let config = StrategiesConfig {
            python_dir: "strategies".to_string(),
            hot_reload: false,
            hot_reload_config: HotReloadConfig::default(),
            python: vec![PythonStrategyConfig {
                id: "MyStrategy".to_string(), // Uppercase not allowed
                file: "my_strategy.py".to_string(),
                class_name: "MyStrategy".to_string(),
                description: "Test".to_string(),
                enabled: true,
                sha256: "abc".to_string(),
                limits: PythonStrategyLimits::default(),
            }],
        };
        let result = ConfigValidator::validate_python_strategies(&config);
        assert!(!result.valid);
        assert!(result.errors[0].message.contains("lowercase"));
    }

    #[test]
    fn test_validate_python_strategy_invalid_file() {
        let config = StrategiesConfig {
            python_dir: "strategies".to_string(),
            hot_reload: false,
            hot_reload_config: HotReloadConfig::default(),
            python: vec![PythonStrategyConfig {
                id: "my_strategy".to_string(),
                file: "my_strategy.js".to_string(), // Wrong extension
                class_name: "MyStrategy".to_string(),
                description: "Test".to_string(),
                enabled: true,
                sha256: "abc".to_string(),
                limits: PythonStrategyLimits::default(),
            }],
        };
        let result = ConfigValidator::validate_python_strategies(&config);
        assert!(!result.valid);
        assert!(result.errors[0].message.contains(".py"));
    }

    #[test]
    fn test_validate_execution_valid() {
        let config = ExecutionConfig::default();
        let result = ConfigValidator::validate_execution(&config);
        assert!(result.valid);
    }

    #[test]
    fn test_validate_execution_invalid_fee_model() {
        let config = ExecutionConfig {
            fees: FeesConfig {
                default_model: "invalid_model".to_string(),
                ..Default::default()
            },
            latency: LatencyConfig::default(),
        };
        let result = ConfigValidator::validate_execution(&config);
        assert!(!result.valid);
        assert!(result.errors[0].message.contains("Invalid fee model"));
    }

    #[test]
    fn test_validate_alerting_valid() {
        let config = AlertingConfig::default();
        let result = ConfigValidator::validate_alerting(&config);
        assert!(result.valid);
    }

    #[test]
    fn test_validate_alerting_invalid_thresholds() {
        let config = AlertingConfig {
            pool_saturation_threshold: 0.95,  // Higher than critical
            pool_critical_threshold: 0.80,
            ..Default::default()
        };
        let result = ConfigValidator::validate_alerting(&config);
        assert!(!result.valid);
    }

    #[test]
    fn test_validate_full_config() {
        let config = AppConfig::default();
        let result = ConfigValidator::validate(&config);
        assert!(result.valid);
    }

    #[test]
    fn test_warning_for_high_rsi_period() {
        let config = BuiltinStrategiesConfig {
            rsi: RsiStrategyConfig {
                period: 250, // Very high
                ..Default::default()
            },
            sma: SmaStrategyConfig::default(),
        };
        let result = ConfigValidator::validate_builtin_strategies(&config);
        assert!(result.valid); // Warnings don't fail validation
        assert!(!result.warnings.is_empty());
    }
}
