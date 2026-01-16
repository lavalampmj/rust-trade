// validator_tests.rs - Tests for TickData validation

use chrono::Utc;
use rust_decimal::Decimal;
use std::str::FromStr;

use super::validator::{TickValidator, ValidationConfig};
use super::types::{TickData, TradeSide};

#[test]
fn test_validate_positive_price() {
    let config = ValidationConfig::default();
    let validator = TickValidator::new(config);

    let tick = TickData::new_unchecked(
        Utc::now(),
        "BTCUSDT".to_string(),
        Decimal::from_str("95000.0").unwrap(),
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );

    // First tick should pass (no previous price)
    assert!(validator.validate(&tick).is_ok());
}

#[test]
fn test_reject_zero_price() {
    let config = ValidationConfig::default();
    let validator = TickValidator::new(config);

    let tick = TickData::new_unchecked(
        Utc::now(),
        "BTCUSDT".to_string(),
        Decimal::ZERO,
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );

    let result = validator.validate(&tick);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("positive"));
}

#[test]
fn test_reject_negative_price() {
    let config = ValidationConfig::default();
    let validator = TickValidator::new(config);

    let tick = TickData::new_unchecked(
        Utc::now(),
        "BTCUSDT".to_string(),
        Decimal::from_str("-100.0").unwrap(),
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );

    let result = validator.validate(&tick);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("positive"));
}

#[test]
fn test_reject_zero_quantity() {
    let config = ValidationConfig::default();
    let validator = TickValidator::new(config);

    let tick = TickData::new_unchecked(
        Utc::now(),
        "BTCUSDT".to_string(),
        Decimal::from_str("95000.0").unwrap(),
        Decimal::ZERO,
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );

    let result = validator.validate(&tick);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Quantity"));
}

#[test]
fn test_reject_empty_symbol() {
    let config = ValidationConfig::default();
    let validator = TickValidator::new(config);

    let tick = TickData::new_unchecked(
        Utc::now(),
        "".to_string(),
        Decimal::from_str("95000.0").unwrap(),
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );

    let result = validator.validate(&tick);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Symbol"));
}

#[test]
fn test_reject_symbol_too_long() {
    let config = ValidationConfig::default();
    let validator = TickValidator::new(config);

    let tick = TickData::new_unchecked(
        Utc::now(),
        "VERYLONGSYMBOLNAMETHATEXCEEDSLIMIT".to_string(), // > 20 chars
        Decimal::from_str("95000.0").unwrap(),
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );

    let result = validator.validate(&tick);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("too long"));
}

#[test]
fn test_reject_symbol_with_special_chars() {
    let config = ValidationConfig::default();
    let validator = TickValidator::new(config);

    let tick = TickData::new_unchecked(
        Utc::now(),
        "BTC-USDT".to_string(), // Contains hyphen
        Decimal::from_str("95000.0").unwrap(),
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );

    let result = validator.validate(&tick);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("alphanumeric"));
}

#[test]
fn test_reject_lowercase_symbol() {
    let config = ValidationConfig::default();
    let validator = TickValidator::new(config);

    let tick = TickData::new_unchecked(
        Utc::now(),
        "btcusdt".to_string(), // lowercase
        Decimal::from_str("95000.0").unwrap(),
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );

    let result = validator.validate(&tick);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("uppercase"));
}

#[test]
fn test_reject_empty_trade_id() {
    let config = ValidationConfig::default();
    let validator = TickValidator::new(config);

    let tick = TickData::new_unchecked(
        Utc::now(),
        "BTCUSDT".to_string(),
        Decimal::from_str("95000.0").unwrap(),
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "".to_string(), // Empty trade ID
        false,
    );

    let result = validator.validate(&tick);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Trade ID"));
}

#[test]
fn test_accept_normal_price_change() {
    let config = ValidationConfig::default(); // 10% default limit
    let validator = TickValidator::new(config);

    // First tick
    let tick1 = TickData::new_unchecked(
        Utc::now(),
        "BTCUSDT".to_string(),
        Decimal::from_str("95000.0").unwrap(),
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );
    assert!(validator.validate(&tick1).is_ok());

    // Second tick: 5% increase (within 10% limit)
    let tick2 = TickData::new_unchecked(
        Utc::now(),
        "BTCUSDT".to_string(),
        Decimal::from_str("99750.0").unwrap(), // +5%
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12346".to_string(),
        false,
    );
    assert!(validator.validate(&tick2).is_ok());
}

#[test]
fn test_reject_excessive_price_change() {
    let config = ValidationConfig::default(); // 10% default limit
    let validator = TickValidator::new(config);

    // First tick
    let tick1 = TickData::new_unchecked(
        Utc::now(),
        "BTCUSDT".to_string(),
        Decimal::from_str("95000.0").unwrap(),
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );
    assert!(validator.validate(&tick1).is_ok());

    // Second tick: 50% drop (exceeds 10% limit) - flash crash
    let tick2 = TickData::new_unchecked(
        Utc::now(),
        "BTCUSDT".to_string(),
        Decimal::from_str("47500.0").unwrap(), // -50%
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12346".to_string(),
        false,
    );

    let result = validator.validate(&tick2);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Price change too large"));
}

#[test]
fn test_different_symbols_independent() {
    let config = ValidationConfig::default();
    let validator = TickValidator::new(config);

    // BTC tick
    let btc_tick = TickData::new_unchecked(
        Utc::now(),
        "BTCUSDT".to_string(),
        Decimal::from_str("95000.0").unwrap(),
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );
    assert!(validator.validate(&btc_tick).is_ok());

    // ETH tick (different symbol, should not affect BTC validation)
    let eth_tick = TickData::new_unchecked(
        Utc::now(),
        "ETHUSDT".to_string(),
        Decimal::from_str("3500.0").unwrap(),
        Decimal::from_str("1.0").unwrap(),
        TradeSide::Buy,
        "67890".to_string(),
        false,
    );
    assert!(validator.validate(&eth_tick).is_ok());

    // Another BTC tick with large change should still be rejected
    let btc_tick2 = TickData::new_unchecked(
        Utc::now(),
        "BTCUSDT".to_string(),
        Decimal::from_str("50000.0").unwrap(), // -47% from first BTC tick
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12347".to_string(),
        false,
    );
    assert!(validator.validate(&btc_tick2).is_err());
}

#[test]
fn test_symbol_specific_override() {
    let mut config = ValidationConfig::default();
    config.set_symbol_override("DOGEUSDT", Decimal::from(20)); // 20% for DOGE

    let validator = TickValidator::new(config);

    // First DOGE tick
    let tick1 = TickData::new_unchecked(
        Utc::now(),
        "DOGEUSDT".to_string(),
        Decimal::from_str("0.08").unwrap(),
        Decimal::from_str("1000.0").unwrap(),
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );
    assert!(validator.validate(&tick1).is_ok());

    // Second DOGE tick: 18% increase (would fail with 10% default, but passes with 20% override)
    let tick2 = TickData::new_unchecked(
        Utc::now(),
        "DOGEUSDT".to_string(),
        Decimal::from_str("0.0944").unwrap(), // +18%
        Decimal::from_str("1000.0").unwrap(),
        TradeSide::Buy,
        "12346".to_string(),
        false,
    );
    assert!(validator.validate(&tick2).is_ok());
}

#[test]
fn test_reject_future_timestamp() {
    let config = ValidationConfig::default();
    let validator = TickValidator::new(config);

    // 10 minutes in the future (exceeds 5-minute tolerance)
    let future_time = Utc::now() + chrono::Duration::minutes(10);

    let tick = TickData::new_unchecked(
        future_time,
        "BTCUSDT".to_string(),
        Decimal::from_str("95000.0").unwrap(),
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );

    let result = validator.validate(&tick);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("future"));
}

#[test]
fn test_accept_recent_past_timestamp() {
    let config = ValidationConfig::default();
    let validator = TickValidator::new(config);

    // 1 minute in the past (within tolerance)
    let past_time = Utc::now() - chrono::Duration::minutes(1);

    let tick = TickData::new_unchecked(
        past_time,
        "BTCUSDT".to_string(),
        Decimal::from_str("95000.0").unwrap(),
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );

    assert!(validator.validate(&tick).is_ok());
}

#[test]
fn test_reject_ancient_timestamp() {
    let config = ValidationConfig::default();
    let validator = TickValidator::new(config);

    // 20 years in the past
    let ancient_time = Utc::now() - chrono::Duration::days(365 * 20);

    let tick = TickData::new_unchecked(
        ancient_time,
        "BTCUSDT".to_string(),
        Decimal::from_str("95000.0").unwrap(),
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );

    let result = validator.validate(&tick);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("past"));
}

#[test]
fn test_validation_disabled() {
    let mut config = ValidationConfig::default();
    config.enabled = false;

    let validator = TickValidator::new(config);

    // This would normally fail (negative price)
    let tick = TickData::new_unchecked(
        Utc::now(),
        "BTCUSDT".to_string(),
        Decimal::from_str("-100.0").unwrap(),
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "12345".to_string(),
        false,
    );

    // But should pass when validation is disabled
    assert!(validator.validate(&tick).is_ok());
}
