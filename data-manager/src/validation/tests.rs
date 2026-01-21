//! Tests for tick data validation

use super::*;
use crate::schema::{NormalizedTick, TradeSide};
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Helper to create a valid test tick
fn create_valid_tick() -> NormalizedTick {
    NormalizedTick::with_details(
        Utc::now(),
        Utc::now(),
        "BTCUSDT".to_string(),
        "BINANCE".to_string(),
        dec!(50000.00),
        dec!(1.5),
        TradeSide::Buy,
        "binance".to_string(),
        Some("12345".to_string()),
        Some(false),
        1,
    )
}

#[test]
fn test_valid_tick_passes_validation() {
    let validator = TickValidator::new();
    let tick = create_valid_tick();

    let result = validator.validate(&tick);
    assert!(result.is_ok(), "Valid tick should pass validation");
}

#[test]
fn test_positive_price_passes() {
    let validator = TickValidator::new();
    let tick = create_valid_tick();

    let result = validator.validate(&tick);
    assert!(result.is_ok());
}

#[test]
fn test_zero_price_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.price = Decimal::ZERO;

    let result = validator.validate(&tick);
    assert!(matches!(
        result,
        Err(ValidationError::PriceNotPositive { .. })
    ));
}

#[test]
fn test_negative_price_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.price = dec!(-100.00);

    let result = validator.validate(&tick);
    assert!(matches!(
        result,
        Err(ValidationError::PriceNotPositive { .. })
    ));
}

#[test]
fn test_price_out_of_bounds_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.price = dec!(999_999_999.00); // Exceeds default max of $10M

    let result = validator.validate(&tick);
    assert!(matches!(
        result,
        Err(ValidationError::PriceOutOfBounds { .. })
    ));
}

#[test]
fn test_positive_size_passes() {
    let validator = TickValidator::new();
    let tick = create_valid_tick();

    let result = validator.validate(&tick);
    assert!(result.is_ok());
}

#[test]
fn test_zero_size_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.size = Decimal::ZERO;

    let result = validator.validate(&tick);
    assert!(matches!(
        result,
        Err(ValidationError::SizeNotPositive { .. })
    ));
}

#[test]
fn test_negative_size_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.size = dec!(-1.0);

    let result = validator.validate(&tick);
    assert!(matches!(
        result,
        Err(ValidationError::SizeNotPositive { .. })
    ));
}

#[test]
fn test_empty_symbol_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.symbol = String::new();

    let result = validator.validate(&tick);
    assert!(matches!(result, Err(ValidationError::EmptySymbol)));
}

#[test]
fn test_symbol_too_long_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.symbol = "ABCDEFGHIJKLMNOPQRSTUVWXYZ".to_string(); // 26 chars, exceeds 20

    let result = validator.validate(&tick);
    assert!(matches!(result, Err(ValidationError::SymbolTooLong { .. })));
}

#[test]
fn test_symbol_with_special_chars_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.symbol = "BTC-USDT".to_string();

    let result = validator.validate(&tick);
    assert!(matches!(
        result,
        Err(ValidationError::InvalidSymbolChars { .. })
    ));
}

#[test]
fn test_lowercase_symbol_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.symbol = "btcusdt".to_string();

    let result = validator.validate(&tick);
    assert!(matches!(
        result,
        Err(ValidationError::SymbolNotUppercase { .. })
    ));
}

#[test]
fn test_empty_exchange_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.exchange = String::new();

    let result = validator.validate(&tick);
    assert!(matches!(result, Err(ValidationError::EmptyExchange)));
}

#[test]
fn test_exchange_too_long_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.exchange = "ABCDEFGHIJKLMNOPQRSTUVWXYZ".to_string(); // 26 chars, exceeds 20

    let result = validator.validate(&tick);
    assert!(matches!(
        result,
        Err(ValidationError::ExchangeTooLong { .. })
    ));
}

#[test]
fn test_exchange_with_underscore_passes() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.exchange = "CME_GLOBEX".to_string();

    let result = validator.validate(&tick);
    assert!(result.is_ok());
}

#[test]
fn test_exchange_with_invalid_chars_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.exchange = "CME-GLOBEX".to_string();

    let result = validator.validate(&tick);
    assert!(matches!(
        result,
        Err(ValidationError::InvalidExchangeChars { .. })
    ));
}

#[test]
fn test_empty_trade_id_passes_when_not_required() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.provider_trade_id = None;

    let result = validator.validate(&tick);
    assert!(result.is_ok());
}

#[test]
fn test_empty_trade_id_fails_when_required() {
    let config = ValidationConfig {
        require_trade_id: true,
        ..Default::default()
    };
    let validator = TickValidator::with_config(config);
    let mut tick = create_valid_tick();
    tick.provider_trade_id = None;

    let result = validator.validate(&tick);
    assert!(matches!(result, Err(ValidationError::EmptyTradeId)));
}

#[test]
fn test_trade_id_too_long_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.provider_trade_id = Some("a".repeat(101)); // Exceeds 100 char limit

    let result = validator.validate(&tick);
    assert!(matches!(result, Err(ValidationError::TradeIdTooLong { .. })));
}

#[test]
fn test_trade_id_with_control_chars_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.provider_trade_id = Some("trade\x00id".to_string());

    let result = validator.validate(&tick);
    assert!(matches!(
        result,
        Err(ValidationError::InvalidTradeIdChars { .. })
    ));
}

#[test]
fn test_suspicious_price_change_detected() {
    let validator = TickValidator::new();

    // First tick establishes baseline
    let tick1 = create_valid_tick();
    validator.validate(&tick1).unwrap();

    // Second tick with >50% price change
    let mut tick2 = create_valid_tick();
    tick2.price = dec!(100000.00); // 100% increase from 50000

    let result = validator.validate(&tick2);
    assert!(matches!(
        result,
        Err(ValidationError::SuspiciousPriceChange { .. })
    ));
}

#[test]
fn test_normal_price_change_passes() {
    let validator = TickValidator::new();

    // First tick establishes baseline
    let tick1 = create_valid_tick();
    validator.validate(&tick1).unwrap();

    // Second tick with 10% price change (under 50% threshold)
    let mut tick2 = create_valid_tick();
    tick2.price = dec!(55000.00); // 10% increase

    let result = validator.validate(&tick2);
    assert!(result.is_ok());
}

#[test]
fn test_symbol_specific_price_threshold() {
    let config = ValidationConfig::default().with_symbol_override("BTCUSDT", dec!(5)); // 5% threshold for BTC

    let validator = TickValidator::with_config(config);

    // First tick establishes baseline
    let tick1 = create_valid_tick();
    validator.validate(&tick1).unwrap();

    // Second tick with 10% price change (exceeds 5% threshold)
    let mut tick2 = create_valid_tick();
    tick2.price = dec!(55000.00);

    let result = validator.validate(&tick2);
    assert!(matches!(
        result,
        Err(ValidationError::SuspiciousPriceChange { .. })
    ));
}

#[test]
fn test_timestamp_in_future_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.ts_event = Utc::now() + Duration::minutes(5); // 5 minutes in future

    let result = validator.validate(&tick);
    assert!(matches!(
        result,
        Err(ValidationError::TimestampInFuture { .. })
    ));
}

#[test]
fn test_timestamp_too_old_fails() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.ts_event = Utc::now() - Duration::hours(48); // 2 days old

    let result = validator.validate(&tick);
    assert!(matches!(
        result,
        Err(ValidationError::TimestampTooOld { .. })
    ));
}

#[test]
fn test_timestamp_within_bounds_passes() {
    let validator = TickValidator::new();
    let mut tick = create_valid_tick();
    tick.ts_event = Utc::now() - Duration::hours(12); // 12 hours old (within 24 hour limit)

    let result = validator.validate(&tick);
    assert!(result.is_ok());
}

#[test]
fn test_validation_disabled_passes_all() {
    let config = ValidationConfig::disabled();
    let validator = TickValidator::with_config(config);

    let mut tick = create_valid_tick();
    tick.price = dec!(-100.00); // Invalid price
    tick.symbol = String::new(); // Invalid symbol

    // Should still pass because validation is disabled
    let result = validator.validate(&tick);
    assert!(result.is_ok());
}

#[test]
fn test_get_last_price() {
    let validator = TickValidator::new();

    // Initially no last price
    assert!(validator.get_last_price("BTCUSDT").is_none());

    // After validation, last price should be set
    let tick = create_valid_tick();
    validator.validate(&tick).unwrap();

    let last_price = validator.get_last_price("BTCUSDT");
    assert_eq!(last_price, Some(dec!(50000.00)));
}

#[test]
fn test_clear_price_history() {
    let validator = TickValidator::new();

    let tick = create_valid_tick();
    validator.validate(&tick).unwrap();
    assert!(validator.get_last_price("BTCUSDT").is_some());

    validator.clear_price_history();
    assert!(validator.get_last_price("BTCUSDT").is_none());
}

#[test]
fn test_validate_readonly_does_not_update_state() {
    let validator = TickValidator::new();

    let tick = create_valid_tick();
    validator.validate_readonly(&tick).unwrap();

    // Last price should not be set
    assert!(validator.get_last_price("BTCUSDT").is_none());
}

#[test]
fn test_multiple_symbols_tracked_independently() {
    let validator = TickValidator::new();

    // Validate BTC tick
    let btc_tick = create_valid_tick();
    validator.validate(&btc_tick).unwrap();

    // Validate ETH tick
    let mut eth_tick = create_valid_tick();
    eth_tick.symbol = "ETHUSDT".to_string();
    eth_tick.price = dec!(3000.00);
    validator.validate(&eth_tick).unwrap();

    // Both prices should be tracked
    assert_eq!(validator.get_last_price("BTCUSDT"), Some(dec!(50000.00)));
    assert_eq!(validator.get_last_price("ETHUSDT"), Some(dec!(3000.00)));

    // Large change in ETH shouldn't affect BTC validation
    let mut eth_tick2 = create_valid_tick();
    eth_tick2.symbol = "ETHUSDT".to_string();
    eth_tick2.price = dec!(6000.00); // 100% increase - should fail

    let result = validator.validate(&eth_tick2);
    assert!(matches!(
        result,
        Err(ValidationError::SuspiciousPriceChange { .. })
    ));
}

#[test]
fn test_config_get_max_price_change() {
    let config =
        ValidationConfig::default().with_symbol_override("BTCUSDT", dec!(5)); // 5% for BTC

    assert_eq!(config.get_max_price_change("BTCUSDT"), dec!(5));
    assert_eq!(config.get_max_price_change("ETHUSDT"), dec!(50)); // Default
}

#[test]
fn test_validation_error_display() {
    let error = ValidationError::PriceNotPositive { price: dec!(-10) };
    let display = format!("{}", error);
    assert!(display.contains("-10"));
    assert!(display.contains("positive"));
}
