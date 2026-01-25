//! Symbol validation utilities.
//!
//! Provides reusable symbol validation for use across providers and validators.

use thiserror::Error;

/// Errors from symbol validation.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum SymbolValidationError {
    /// Symbol is empty
    #[error("symbol cannot be empty")]
    Empty,

    /// Symbol is too short
    #[error("symbol '{symbol}' is too short (min {min} characters)")]
    TooShort { symbol: String, min: usize },

    /// Symbol is too long
    #[error("symbol '{symbol}' exceeds maximum length of {max} characters")]
    TooLong { symbol: String, max: usize },

    /// Symbol contains invalid characters
    #[error("symbol '{symbol}' contains invalid characters (must be alphanumeric)")]
    InvalidCharacters { symbol: String },

    /// Symbol is not uppercase
    #[error("symbol '{symbol}' must be uppercase")]
    NotUppercase { symbol: String },
}

/// Configuration for symbol validation.
#[derive(Debug, Clone)]
pub struct SymbolValidatorConfig {
    /// Minimum symbol length (default: 1)
    pub min_length: usize,
    /// Maximum symbol length (default: 20)
    pub max_length: usize,
    /// Whether to require uppercase (default: true)
    pub require_uppercase: bool,
    /// Whether to allow only alphanumeric characters (default: true)
    pub alphanumeric_only: bool,
}

impl Default for SymbolValidatorConfig {
    fn default() -> Self {
        Self {
            min_length: 1,
            max_length: 20,
            require_uppercase: true,
            alphanumeric_only: true,
        }
    }
}

impl SymbolValidatorConfig {
    /// Create config for Binance-style symbols (3-20 chars, alphanumeric, uppercase)
    pub fn binance() -> Self {
        Self {
            min_length: 3,
            max_length: 20,
            require_uppercase: true,
            alphanumeric_only: true,
        }
    }

    /// Create config for flexible validation (1-20 chars, uppercase)
    pub fn flexible() -> Self {
        Self {
            min_length: 1,
            max_length: 20,
            require_uppercase: true,
            alphanumeric_only: true,
        }
    }

    /// Set minimum length
    pub fn with_min_length(mut self, min: usize) -> Self {
        self.min_length = min;
        self
    }

    /// Set maximum length
    pub fn with_max_length(mut self, max: usize) -> Self {
        self.max_length = max;
        self
    }

    /// Set uppercase requirement
    pub fn with_require_uppercase(mut self, require: bool) -> Self {
        self.require_uppercase = require;
        self
    }
}

/// Symbol validator with configurable rules.
///
/// Provides both validation-only and normalize-and-validate operations.
///
/// # Example
///
/// ```
/// use trading_common::validation::{SymbolValidator, SymbolValidatorConfig};
///
/// // Default validator
/// let validator = SymbolValidator::new();
/// assert!(validator.validate("BTCUSDT").is_ok());
/// assert!(validator.validate("btcusdt").is_err()); // Not uppercase
///
/// // Normalize input (converts to uppercase)
/// let normalized = validator.normalize("btcusdt").unwrap();
/// assert_eq!(normalized, "BTCUSDT");
///
/// // Binance-style validator (min 3 chars)
/// let binance = SymbolValidator::binance();
/// assert!(binance.validate("BT").is_err()); // Too short
/// ```
#[derive(Debug, Clone)]
pub struct SymbolValidator {
    config: SymbolValidatorConfig,
}

impl SymbolValidator {
    /// Create a new validator with default config.
    pub fn new() -> Self {
        Self {
            config: SymbolValidatorConfig::default(),
        }
    }

    /// Create a new validator with custom config.
    pub fn with_config(config: SymbolValidatorConfig) -> Self {
        Self { config }
    }

    /// Create a Binance-style validator (3-20 chars, alphanumeric, uppercase).
    pub fn binance() -> Self {
        Self::with_config(SymbolValidatorConfig::binance())
    }

    /// Validate a symbol without modifying it.
    ///
    /// Returns `Ok(())` if valid, `Err` with details if invalid.
    pub fn validate(&self, symbol: &str) -> Result<(), SymbolValidationError> {
        // Check empty
        if symbol.is_empty() {
            return Err(SymbolValidationError::Empty);
        }

        // Check min length
        if symbol.len() < self.config.min_length {
            return Err(SymbolValidationError::TooShort {
                symbol: symbol.to_string(),
                min: self.config.min_length,
            });
        }

        // Check max length
        if symbol.len() > self.config.max_length {
            return Err(SymbolValidationError::TooLong {
                symbol: symbol.to_string(),
                max: self.config.max_length,
            });
        }

        // Check characters
        if self.config.alphanumeric_only && !symbol.chars().all(|c| c.is_alphanumeric()) {
            return Err(SymbolValidationError::InvalidCharacters {
                symbol: symbol.to_string(),
            });
        }

        // Check uppercase
        if self.config.require_uppercase && symbol != symbol.to_uppercase() {
            return Err(SymbolValidationError::NotUppercase {
                symbol: symbol.to_string(),
            });
        }

        Ok(())
    }

    /// Normalize and validate a symbol.
    ///
    /// Converts to uppercase (if required), then validates.
    /// Returns the normalized symbol if valid.
    pub fn normalize(&self, symbol: &str) -> Result<String, SymbolValidationError> {
        // Check empty first
        if symbol.is_empty() {
            return Err(SymbolValidationError::Empty);
        }

        // Normalize to uppercase if required
        let normalized = if self.config.require_uppercase {
            symbol.to_uppercase()
        } else {
            symbol.to_string()
        };

        // Check min length
        if normalized.len() < self.config.min_length {
            return Err(SymbolValidationError::TooShort {
                symbol: normalized,
                min: self.config.min_length,
            });
        }

        // Check max length
        if normalized.len() > self.config.max_length {
            return Err(SymbolValidationError::TooLong {
                symbol: normalized,
                max: self.config.max_length,
            });
        }

        // Check characters
        if self.config.alphanumeric_only && !normalized.chars().all(|c| c.is_alphanumeric()) {
            return Err(SymbolValidationError::InvalidCharacters {
                symbol: normalized,
            });
        }

        Ok(normalized)
    }

    /// Check if a symbol is valid (convenience method).
    pub fn is_valid(&self, symbol: &str) -> bool {
        self.validate(symbol).is_ok()
    }

    /// Get the current config.
    pub fn config(&self) -> &SymbolValidatorConfig {
        &self.config
    }
}

impl Default for SymbolValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_symbol() {
        let validator = SymbolValidator::new();
        assert!(validator.validate("BTCUSDT").is_ok());
        assert!(validator.validate("A").is_ok());
        assert!(validator.validate("ES1").is_ok());
    }

    #[test]
    fn test_validate_empty() {
        let validator = SymbolValidator::new();
        assert_eq!(validator.validate(""), Err(SymbolValidationError::Empty));
    }

    #[test]
    fn test_validate_too_short() {
        let validator = SymbolValidator::binance(); // min 3 chars
        let result = validator.validate("BT");
        assert!(matches!(result, Err(SymbolValidationError::TooShort { .. })));
    }

    #[test]
    fn test_validate_too_long() {
        let validator = SymbolValidator::with_config(
            SymbolValidatorConfig::default().with_max_length(5),
        );
        let result = validator.validate("BTCUSDT");
        assert!(matches!(result, Err(SymbolValidationError::TooLong { .. })));
    }

    #[test]
    fn test_validate_invalid_characters() {
        let validator = SymbolValidator::new();
        let result = validator.validate("BTC-USDT");
        assert!(matches!(
            result,
            Err(SymbolValidationError::InvalidCharacters { .. })
        ));
    }

    #[test]
    fn test_validate_not_uppercase() {
        let validator = SymbolValidator::new();
        let result = validator.validate("btcusdt");
        assert!(matches!(
            result,
            Err(SymbolValidationError::NotUppercase { .. })
        ));
    }

    #[test]
    fn test_normalize_converts_to_uppercase() {
        let validator = SymbolValidator::new();
        let result = validator.normalize("btcusdt").unwrap();
        assert_eq!(result, "BTCUSDT");
    }

    #[test]
    fn test_normalize_validates_after_conversion() {
        let validator = SymbolValidator::new();
        // Invalid characters still fail after uppercase conversion
        let result = validator.normalize("btc-usdt");
        assert!(matches!(
            result,
            Err(SymbolValidationError::InvalidCharacters { .. })
        ));
    }

    #[test]
    fn test_normalize_empty() {
        let validator = SymbolValidator::new();
        assert_eq!(validator.normalize(""), Err(SymbolValidationError::Empty));
    }

    #[test]
    fn test_binance_validator() {
        let validator = SymbolValidator::binance();
        assert!(validator.validate("BTCUSDT").is_ok());
        assert!(validator.validate("BT").is_err()); // Too short (min 3)
        assert!(validator.normalize("btcusdt").is_ok()); // Normalizes to uppercase
    }

    #[test]
    fn test_is_valid() {
        let validator = SymbolValidator::new();
        assert!(validator.is_valid("BTCUSDT"));
        assert!(!validator.is_valid(""));
        assert!(!validator.is_valid("btc"));
    }

    #[test]
    fn test_config_builder() {
        let config = SymbolValidatorConfig::default()
            .with_min_length(2)
            .with_max_length(10)
            .with_require_uppercase(false);

        let validator = SymbolValidator::with_config(config);
        assert!(validator.validate("abc").is_ok()); // lowercase allowed
        assert!(validator.validate("a").is_err()); // too short
    }
}
