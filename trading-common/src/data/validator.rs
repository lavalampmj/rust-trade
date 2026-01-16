// validator.rs - Input validation for TickData

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::types::{DataError, TickData};

/// Validation configuration
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    /// Enable validation (default: true)
    pub enabled: bool,

    /// Maximum price change percentage per tick
    pub max_price_change_pct: Decimal,

    /// Timestamp tolerance in minutes (future)
    pub timestamp_tolerance_minutes: i64,

    /// Maximum past timestamp in days
    pub max_past_days: i64,

    /// Symbol-specific overrides
    pub symbol_overrides: HashMap<String, Decimal>,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_price_change_pct: Decimal::from(10),
            timestamp_tolerance_minutes: 5,
            max_past_days: 365 * 10, // 10 years
            symbol_overrides: HashMap::new(),
        }
    }
}

impl ValidationConfig {
    /// Set symbol-specific price change override
    pub fn set_symbol_override(&mut self, symbol: &str, max_change_pct: Decimal) {
        self.symbol_overrides.insert(symbol.to_string(), max_change_pct);
    }

    /// Get max price change for specific symbol
    fn max_price_change_for_symbol(&self, symbol: &str) -> Decimal {
        self.symbol_overrides
            .get(symbol)
            .copied()
            .unwrap_or(self.max_price_change_pct)
    }
}

/// Stateful validator that tracks last seen prices per symbol
pub struct TickValidator {
    config: ValidationConfig,
    last_prices: Arc<RwLock<HashMap<String, Decimal>>>,
}

impl TickValidator {
    /// Create new validator with configuration
    pub fn new(config: ValidationConfig) -> Self {
        Self {
            config,
            last_prices: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Validate tick data (absolute + relative)
    pub fn validate(&self, tick: &TickData) -> Result<(), DataError> {
        // If validation is disabled, skip all checks
        if !self.config.enabled {
            return Ok(());
        }

        // Absolute validation (always run)
        Self::validate_price_absolute(&tick.price)?;
        Self::validate_quantity_absolute(&tick.quantity)?;
        Self::validate_timestamp(&tick.timestamp, &self.config)?;
        Self::validate_symbol(&tick.symbol)?;
        Self::validate_trade_id(&tick.trade_id)?;

        // Relative validation (if previous price exists)
        let last_prices = self.last_prices.read().unwrap();
        let previous_price = last_prices.get(&tick.symbol).copied();
        drop(last_prices);

        Self::validate_price_relative(
            &tick.price,
            &tick.symbol,
            previous_price,
            &self.config,
        )?;

        // Update last known price
        let mut last_prices = self.last_prices.write().unwrap();
        last_prices.insert(tick.symbol.clone(), tick.price);

        Ok(())
    }

    /// Validate price is positive and within reasonable bounds
    fn validate_price_absolute(price: &Decimal) -> Result<(), DataError> {
        // Must be positive
        if *price <= Decimal::ZERO {
            return Err(DataError::Validation(
                format!("Price must be positive, got: {}", price)
            ));
        }

        // Not unreasonably small (prevents underflow/corruption)
        let absolute_min = Decimal::new(1, 10); // 1e-10
        if *price < absolute_min {
            return Err(DataError::Validation(
                format!("Price too small (likely data corruption): {}", price)
            ));
        }

        // Not unreasonably large (prevents overflow)
        let absolute_max = Decimal::new(i64::MAX, 0);
        if *price > absolute_max {
            return Err(DataError::Validation(
                format!("Price too large (likely data corruption): {}", price)
            ));
        }

        Ok(())
    }

    /// Validate quantity is positive
    fn validate_quantity_absolute(quantity: &Decimal) -> Result<(), DataError> {
        if *quantity <= Decimal::ZERO {
            return Err(DataError::Validation(
                format!("Quantity must be positive, got: {}", quantity)
            ));
        }

        Ok(())
    }

    /// Validate price change against previous tick
    fn validate_price_relative(
        price: &Decimal,
        symbol: &str,
        previous_price: Option<Decimal>,
        config: &ValidationConfig,
    ) -> Result<(), DataError> {
        // If no previous price, skip relative validation (first tick)
        let Some(prev_price) = previous_price else {
            return Ok(());
        };

        // Calculate percentage change: |new - old| / old * 100
        let price_diff = (*price - prev_price).abs();
        let price_change_pct = (price_diff / prev_price) * Decimal::from(100);

        // Get symbol-specific or default limit
        let max_change = config.max_price_change_for_symbol(symbol);

        // Check if change exceeds configured limit
        if price_change_pct > max_change {
            return Err(DataError::Validation(format!(
                "Price change too large for {}: {:.2}% (prev: {}, new: {}, limit: {}%)",
                symbol,
                price_change_pct,
                prev_price,
                price,
                max_change
            )));
        }

        Ok(())
    }

    /// Validate timestamp is not too far in future or past
    fn validate_timestamp(
        timestamp: &DateTime<Utc>,
        config: &ValidationConfig,
    ) -> Result<(), DataError> {
        let now = Utc::now();

        // Not too far in the future (allows for clock skew)
        let max_future = now + Duration::minutes(config.timestamp_tolerance_minutes);
        if *timestamp > max_future {
            return Err(DataError::Validation(format!(
                "Timestamp too far in future (max {} minutes ahead): {}",
                config.timestamp_tolerance_minutes, timestamp
            )));
        }

        // Not too far in the past
        let max_past = now - Duration::days(config.max_past_days);
        if *timestamp < max_past {
            return Err(DataError::Validation(format!(
                "Timestamp too far in past (max {} days ago): {}",
                config.max_past_days, timestamp
            )));
        }

        Ok(())
    }

    /// Validate symbol format and constraints
    fn validate_symbol(symbol: &str) -> Result<(), DataError> {
        const MAX_SYMBOL_LENGTH: usize = 20; // Matches DB VARCHAR(20)
        const MIN_SYMBOL_LENGTH: usize = 3;

        // Not empty
        if symbol.is_empty() {
            return Err(DataError::Validation("Symbol cannot be empty".to_string()));
        }

        // Length constraints
        if symbol.len() < MIN_SYMBOL_LENGTH {
            return Err(DataError::Validation(format!(
                "Symbol too short (min {} chars): {}",
                MIN_SYMBOL_LENGTH, symbol
            )));
        }

        if symbol.len() > MAX_SYMBOL_LENGTH {
            return Err(DataError::Validation(format!(
                "Symbol too long (max {} chars): {}",
                MAX_SYMBOL_LENGTH, symbol
            )));
        }

        // Alphanumeric only
        if !symbol.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(DataError::Validation(format!(
                "Symbol must be alphanumeric only: {}",
                symbol
            )));
        }

        // Uppercase only
        if symbol.chars().any(|c| c.is_ascii_lowercase()) {
            return Err(DataError::Validation(format!(
                "Symbol must be uppercase: {} (expected: {})",
                symbol,
                symbol.to_uppercase()
            )));
        }

        Ok(())
    }

    /// Validate trade ID format
    fn validate_trade_id(trade_id: &str) -> Result<(), DataError> {
        const MAX_TRADE_ID_LENGTH: usize = 64;

        // Not empty
        if trade_id.is_empty() {
            return Err(DataError::Validation("Trade ID cannot be empty".to_string()));
        }

        // Reasonable length
        if trade_id.len() > MAX_TRADE_ID_LENGTH {
            return Err(DataError::Validation(format!(
                "Trade ID too long (max {} chars)",
                MAX_TRADE_ID_LENGTH
            )));
        }

        // Printable ASCII only
        if !trade_id.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
            return Err(DataError::Validation(
                "Trade ID must contain only printable ASCII characters".to_string(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = ValidationConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_price_change_pct, Decimal::from(10));
        assert_eq!(config.timestamp_tolerance_minutes, 5);
    }

    #[test]
    fn test_symbol_override() {
        let mut config = ValidationConfig::default();
        config.set_symbol_override("DOGEUSDT", Decimal::from(20));

        assert_eq!(
            config.max_price_change_for_symbol("DOGEUSDT"),
            Decimal::from(20)
        );
        assert_eq!(
            config.max_price_change_for_symbol("BTCUSDT"),
            Decimal::from(10)
        ); // Falls back to default
    }
}
