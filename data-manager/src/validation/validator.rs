//! Tick data validator for NormalizedTick
//!
//! Validates incoming tick data for data integrity before storage or IPC distribution.

use crate::schema::NormalizedTick;
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Validation errors for tick data
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationError {
    /// Price must be positive
    PriceNotPositive { price: Decimal },
    /// Price exceeds reasonable bounds
    PriceOutOfBounds { price: Decimal, max: Decimal },
    /// Suspicious price change detected
    SuspiciousPriceChange {
        symbol: String,
        old_price: Decimal,
        new_price: Decimal,
        change_percent: Decimal,
    },
    /// Quantity/size must be positive
    SizeNotPositive { size: Decimal },
    /// Symbol is empty
    EmptySymbol,
    /// Symbol exceeds maximum length
    SymbolTooLong { symbol: String, max_length: usize },
    /// Symbol contains invalid characters
    InvalidSymbolChars { symbol: String },
    /// Symbol must be uppercase
    SymbolNotUppercase { symbol: String },
    /// Exchange is empty
    EmptyExchange,
    /// Exchange exceeds maximum length
    ExchangeTooLong { exchange: String, max_length: usize },
    /// Exchange contains invalid characters
    InvalidExchangeChars { exchange: String },
    /// Trade ID is empty when required
    EmptyTradeId,
    /// Trade ID exceeds maximum length
    TradeIdTooLong { trade_id: String, max_length: usize },
    /// Trade ID contains invalid characters
    InvalidTradeIdChars { trade_id: String },
    /// Timestamp is too far in the future
    TimestampInFuture {
        timestamp: chrono::DateTime<Utc>,
        max_future: Duration,
    },
    /// Timestamp is too far in the past
    TimestampTooOld {
        timestamp: chrono::DateTime<Utc>,
        max_age: Duration,
    },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::PriceNotPositive { price } => {
                write!(f, "Price must be positive, got: {}", price)
            }
            ValidationError::PriceOutOfBounds { price, max } => {
                write!(f, "Price {} exceeds maximum bound {}", price, max)
            }
            ValidationError::SuspiciousPriceChange {
                symbol,
                old_price,
                new_price,
                change_percent,
            } => {
                write!(
                    f,
                    "Suspicious price change for {}: {} -> {} ({}%)",
                    symbol, old_price, new_price, change_percent
                )
            }
            ValidationError::SizeNotPositive { size } => {
                write!(f, "Size must be positive, got: {}", size)
            }
            ValidationError::EmptySymbol => write!(f, "Symbol cannot be empty"),
            ValidationError::SymbolTooLong { symbol, max_length } => {
                write!(
                    f,
                    "Symbol '{}' exceeds maximum length of {}",
                    symbol, max_length
                )
            }
            ValidationError::InvalidSymbolChars { symbol } => {
                write!(
                    f,
                    "Symbol '{}' contains invalid characters (must be alphanumeric)",
                    symbol
                )
            }
            ValidationError::SymbolNotUppercase { symbol } => {
                write!(f, "Symbol '{}' must be uppercase", symbol)
            }
            ValidationError::EmptyExchange => write!(f, "Exchange cannot be empty"),
            ValidationError::ExchangeTooLong {
                exchange,
                max_length,
            } => {
                write!(
                    f,
                    "Exchange '{}' exceeds maximum length of {}",
                    exchange, max_length
                )
            }
            ValidationError::InvalidExchangeChars { exchange } => {
                write!(
                    f,
                    "Exchange '{}' contains invalid characters (must be alphanumeric or underscore)",
                    exchange
                )
            }
            ValidationError::EmptyTradeId => write!(f, "Trade ID cannot be empty"),
            ValidationError::TradeIdTooLong {
                trade_id,
                max_length,
            } => {
                write!(
                    f,
                    "Trade ID '{}' exceeds maximum length of {}",
                    trade_id, max_length
                )
            }
            ValidationError::InvalidTradeIdChars { trade_id } => {
                write!(
                    f,
                    "Trade ID '{}' contains invalid characters (must be printable ASCII)",
                    trade_id
                )
            }
            ValidationError::TimestampInFuture {
                timestamp,
                max_future,
            } => {
                write!(
                    f,
                    "Timestamp {} is more than {:?} in the future",
                    timestamp, max_future
                )
            }
            ValidationError::TimestampTooOld { timestamp, max_age } => {
                write!(
                    f,
                    "Timestamp {} is more than {:?} in the past",
                    timestamp, max_age
                )
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Result type for validation operations
pub type ValidationResult<T> = Result<T, ValidationError>;

/// Configuration for tick validation
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    /// Maximum allowed price (for sanity check)
    pub max_price: Decimal,
    /// Maximum allowed percentage change from last known price
    pub max_price_change_percent: Decimal,
    /// Maximum symbol length
    pub max_symbol_length: usize,
    /// Maximum exchange length
    pub max_exchange_length: usize,
    /// Maximum trade ID length
    pub max_trade_id_length: usize,
    /// Maximum timestamp age (how old a tick can be)
    pub max_timestamp_age: Duration,
    /// Maximum future timestamp allowed (clock skew tolerance)
    pub max_timestamp_future: Duration,
    /// Whether to require trade IDs
    pub require_trade_id: bool,
    /// Whether validation is enabled at all
    pub enabled: bool,
    /// Symbol-specific price change thresholds (overrides default)
    pub symbol_overrides: HashMap<String, Decimal>,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            max_price: Decimal::from(10_000_000), // $10M max
            max_price_change_percent: Decimal::from(50), // 50% max change
            max_symbol_length: 20,
            max_exchange_length: 20,
            max_trade_id_length: 100,
            max_timestamp_age: Duration::hours(24),   // 24 hours old max
            max_timestamp_future: Duration::seconds(60), // 60 seconds future tolerance
            require_trade_id: false,
            enabled: true,
            symbol_overrides: HashMap::new(),
        }
    }
}

impl ValidationConfig {
    /// Create a new validation config with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a config with validation disabled
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }

    /// Set symbol-specific price change threshold
    pub fn with_symbol_override(mut self, symbol: &str, max_change_percent: Decimal) -> Self {
        self.symbol_overrides
            .insert(symbol.to_string(), max_change_percent);
        self
    }

    /// Get the max price change threshold for a symbol
    pub fn get_max_price_change(&self, symbol: &str) -> Decimal {
        self.symbol_overrides
            .get(symbol)
            .copied()
            .unwrap_or(self.max_price_change_percent)
    }
}

/// Tick data validator
///
/// Validates incoming ticks for data integrity. Tracks last known prices
/// per symbol to detect suspicious price changes.
pub struct TickValidator {
    config: ValidationConfig,
    last_prices: Arc<RwLock<HashMap<String, Decimal>>>,
}

impl TickValidator {
    /// Create a new validator with default config
    pub fn new() -> Self {
        Self {
            config: ValidationConfig::default(),
            last_prices: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a validator with custom config
    pub fn with_config(config: ValidationConfig) -> Self {
        Self {
            config,
            last_prices: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Validate a tick and update internal state
    pub fn validate(&self, tick: &NormalizedTick) -> ValidationResult<()> {
        if !self.config.enabled {
            return Ok(());
        }

        self.validate_price(tick)?;
        self.validate_size(tick)?;
        self.validate_symbol(tick)?;
        self.validate_exchange(tick)?;
        self.validate_trade_id(tick)?;
        self.validate_timestamp(tick)?;
        self.check_price_change(tick)?;

        // Update last price after successful validation
        self.update_last_price(tick);

        Ok(())
    }

    /// Validate without updating state (for preview/dry-run)
    pub fn validate_readonly(&self, tick: &NormalizedTick) -> ValidationResult<()> {
        if !self.config.enabled {
            return Ok(());
        }

        self.validate_price(tick)?;
        self.validate_size(tick)?;
        self.validate_symbol(tick)?;
        self.validate_exchange(tick)?;
        self.validate_trade_id(tick)?;
        self.validate_timestamp(tick)?;
        self.check_price_change(tick)?;

        Ok(())
    }

    fn validate_price(&self, tick: &NormalizedTick) -> ValidationResult<()> {
        if tick.price <= Decimal::ZERO {
            return Err(ValidationError::PriceNotPositive { price: tick.price });
        }

        if tick.price > self.config.max_price {
            return Err(ValidationError::PriceOutOfBounds {
                price: tick.price,
                max: self.config.max_price,
            });
        }

        Ok(())
    }

    fn validate_size(&self, tick: &NormalizedTick) -> ValidationResult<()> {
        if tick.size <= Decimal::ZERO {
            return Err(ValidationError::SizeNotPositive { size: tick.size });
        }
        Ok(())
    }

    fn validate_symbol(&self, tick: &NormalizedTick) -> ValidationResult<()> {
        if tick.symbol.is_empty() {
            return Err(ValidationError::EmptySymbol);
        }

        if tick.symbol.len() > self.config.max_symbol_length {
            return Err(ValidationError::SymbolTooLong {
                symbol: tick.symbol.clone(),
                max_length: self.config.max_symbol_length,
            });
        }

        if !tick.symbol.chars().all(|c| c.is_alphanumeric()) {
            return Err(ValidationError::InvalidSymbolChars {
                symbol: tick.symbol.clone(),
            });
        }

        if tick.symbol != tick.symbol.to_uppercase() {
            return Err(ValidationError::SymbolNotUppercase {
                symbol: tick.symbol.clone(),
            });
        }

        Ok(())
    }

    fn validate_exchange(&self, tick: &NormalizedTick) -> ValidationResult<()> {
        if tick.exchange.is_empty() {
            return Err(ValidationError::EmptyExchange);
        }

        if tick.exchange.len() > self.config.max_exchange_length {
            return Err(ValidationError::ExchangeTooLong {
                exchange: tick.exchange.clone(),
                max_length: self.config.max_exchange_length,
            });
        }

        // Allow alphanumeric and underscore for exchange names
        if !tick
            .exchange
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_')
        {
            return Err(ValidationError::InvalidExchangeChars {
                exchange: tick.exchange.clone(),
            });
        }

        Ok(())
    }

    fn validate_trade_id(&self, tick: &NormalizedTick) -> ValidationResult<()> {
        let trade_id = tick.provider_trade_id.as_deref().unwrap_or("");

        if self.config.require_trade_id && trade_id.is_empty() {
            return Err(ValidationError::EmptyTradeId);
        }

        if !trade_id.is_empty() {
            if trade_id.len() > self.config.max_trade_id_length {
                return Err(ValidationError::TradeIdTooLong {
                    trade_id: trade_id.to_string(),
                    max_length: self.config.max_trade_id_length,
                });
            }

            // Trade ID must be printable ASCII
            if !trade_id.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
                return Err(ValidationError::InvalidTradeIdChars {
                    trade_id: trade_id.to_string(),
                });
            }
        }

        Ok(())
    }

    fn validate_timestamp(&self, tick: &NormalizedTick) -> ValidationResult<()> {
        let now = Utc::now();

        // Check if timestamp is too far in the future
        let future_limit = now + self.config.max_timestamp_future;
        if tick.ts_event > future_limit {
            return Err(ValidationError::TimestampInFuture {
                timestamp: tick.ts_event,
                max_future: self.config.max_timestamp_future,
            });
        }

        // Check if timestamp is too old
        let past_limit = now - self.config.max_timestamp_age;
        if tick.ts_event < past_limit {
            return Err(ValidationError::TimestampTooOld {
                timestamp: tick.ts_event,
                max_age: self.config.max_timestamp_age,
            });
        }

        Ok(())
    }

    fn check_price_change(&self, tick: &NormalizedTick) -> ValidationResult<()> {
        let last_prices = self.last_prices.read().unwrap();

        if let Some(last_price) = last_prices.get(&tick.symbol) {
            if *last_price > Decimal::ZERO {
                let change = ((tick.price - *last_price) / *last_price * Decimal::from(100)).abs();
                let max_change = self.config.get_max_price_change(&tick.symbol);

                if change > max_change {
                    return Err(ValidationError::SuspiciousPriceChange {
                        symbol: tick.symbol.clone(),
                        old_price: *last_price,
                        new_price: tick.price,
                        change_percent: change,
                    });
                }
            }
        }

        Ok(())
    }

    fn update_last_price(&self, tick: &NormalizedTick) {
        let mut last_prices = self.last_prices.write().unwrap();
        last_prices.insert(tick.symbol.clone(), tick.price);
    }

    /// Get the last known price for a symbol
    pub fn get_last_price(&self, symbol: &str) -> Option<Decimal> {
        let last_prices = self.last_prices.read().unwrap();
        last_prices.get(symbol).copied()
    }

    /// Clear the last price history
    pub fn clear_price_history(&self) {
        let mut last_prices = self.last_prices.write().unwrap();
        last_prices.clear();
    }

    /// Get the current config
    pub fn config(&self) -> &ValidationConfig {
        &self.config
    }
}

impl Default for TickValidator {
    fn default() -> Self {
        Self::new()
    }
}
