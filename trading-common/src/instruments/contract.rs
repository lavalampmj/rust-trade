//! Contract specifications for derivative instruments.
//!
//! This module provides contract-level metadata for futures, options, and other
//! derivative instruments, aligned with Databento's InstrumentDefMsg schema.
//!
//! # Databento Alignment
//!
//! Key fields from Databento's instrument definition:
//! - `expiration`: Contract expiration timestamp
//! - `activation`: When contract becomes tradeable
//! - `underlying`: Underlying symbol
//! - `unit_of_measure_qty`: Contract multiplier
//! - `strike_price`: Option strike price
//!
//! # Example
//!
//! ```ignore
//! use trading_common::instruments::contract::{ContractSpec, SettlementType};
//! use rust_decimal_macros::dec;
//! use chrono::{DateTime, Utc, NaiveDate};
//!
//! let es_future = ContractSpec::future(
//!     dec!(50),  // Contract multiplier
//!     SettlementType::Cash,
//! )
//! .with_expiration(expiry_datetime)
//! .with_underlying("ES");
//! ```

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

use super::types::OptionType;

/// Contract specifications for derivatives (futures/options).
///
/// Aligns with Databento InstrumentDefMsg contract fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractSpec {
    /// Contract activation date (Databento: activation)
    /// When the contract becomes tradeable
    #[serde(default)]
    pub activation: Option<DateTime<Utc>>,

    /// Contract expiration date (Databento: expiration)
    #[serde(default)]
    pub expiration: Option<DateTime<Utc>>,

    /// Maturity year (Databento: maturity_year)
    #[serde(default)]
    pub maturity_year: Option<u16>,

    /// Maturity month (Databento: maturity_month)
    #[serde(default)]
    pub maturity_month: Option<u8>,

    /// Maturity day (Databento: maturity_day)
    #[serde(default)]
    pub maturity_day: Option<u8>,

    /// Maturity week (Databento: maturity_week)
    #[serde(default)]
    pub maturity_week: Option<u8>,

    /// Contract multiplier (Databento: unit_of_measure_qty)
    /// Notional value per point (e.g., $50 for ES, $10 for NQ)
    pub contract_multiplier: Decimal,

    /// Original contract size (Databento: original_contract_size)
    #[serde(default)]
    pub original_contract_size: Option<i32>,

    /// Unit of measure (Databento: unit_of_measure[31])
    /// Examples: "IPNT" (index points), "BBL" (barrels), "OZ" (ounces)
    #[serde(default)]
    pub unit_of_measure: Option<String>,

    /// Contract multiplier unit (Databento: contract_multiplier_unit)
    #[serde(default)]
    pub contract_multiplier_unit: Option<i8>,

    /// Flow schedule type (Databento: flow_schedule_type)
    #[serde(default)]
    pub flow_schedule_type: Option<i8>,

    /// Underlying symbol (Databento: underlying[21])
    #[serde(default)]
    pub underlying: Option<String>,

    /// Underlying product code (Databento: underlying_product)
    #[serde(default)]
    pub underlying_product: Option<u8>,

    /// Underlying instrument ID (Databento: underlying_id)
    #[serde(default)]
    pub underlying_id: Option<u32>,

    /// Decay quantity (Databento: decay_quantity)
    #[serde(default)]
    pub decay_quantity: Option<i32>,

    /// Decay start date (Databento: decay_start_date)
    #[serde(default)]
    pub decay_start_date: Option<u16>,

    /// Settlement type
    pub settlement: SettlementType,

    /// First notice date (futures physical delivery)
    #[serde(default)]
    pub first_notice_date: Option<NaiveDate>,

    /// Last trading date
    #[serde(default)]
    pub last_trading_date: Option<NaiveDate>,

    /// Final settlement date
    #[serde(default)]
    pub settlement_date: Option<NaiveDate>,

    // --- Options-specific fields ---
    /// Strike price (Databento: strike_price)
    #[serde(default)]
    pub strike_price: Option<Decimal>,

    /// Strike price currency (Databento: strike_price_currency[4])
    #[serde(default)]
    pub strike_price_currency: Option<String>,

    /// Option type (Call/Put)
    #[serde(default)]
    pub option_type: Option<OptionType>,

    /// Exercise style
    #[serde(default)]
    pub exercise_style: Option<ExerciseStyle>,

    // --- Continuous contract fields ---
    /// Roll rule for continuous contracts
    #[serde(default)]
    pub roll_rule: Option<RollRule>,
}

impl ContractSpec {
    /// Create a basic contract spec with multiplier and settlement
    pub fn new(contract_multiplier: Decimal, settlement: SettlementType) -> Self {
        Self {
            activation: None,
            expiration: None,
            maturity_year: None,
            maturity_month: None,
            maturity_day: None,
            maturity_week: None,
            contract_multiplier,
            original_contract_size: None,
            unit_of_measure: None,
            contract_multiplier_unit: None,
            flow_schedule_type: None,
            underlying: None,
            underlying_product: None,
            underlying_id: None,
            decay_quantity: None,
            decay_start_date: None,
            settlement,
            first_notice_date: None,
            last_trading_date: None,
            settlement_date: None,
            strike_price: None,
            strike_price_currency: None,
            option_type: None,
            exercise_style: None,
            roll_rule: None,
        }
    }

    /// Create a futures contract spec
    pub fn future(contract_multiplier: Decimal, settlement: SettlementType) -> Self {
        Self::new(contract_multiplier, settlement)
    }

    /// Create a perpetual contract spec (no expiration)
    pub fn perpetual(contract_multiplier: Decimal) -> Self {
        Self::new(contract_multiplier, SettlementType::Cash)
    }

    /// Create an option contract spec
    pub fn option(
        contract_multiplier: Decimal,
        strike_price: Decimal,
        option_type: OptionType,
        exercise_style: ExerciseStyle,
    ) -> Self {
        Self {
            strike_price: Some(strike_price),
            option_type: Some(option_type),
            exercise_style: Some(exercise_style),
            ..Self::new(contract_multiplier, SettlementType::Cash)
        }
    }

    /// Add activation date
    pub fn with_activation(mut self, activation: DateTime<Utc>) -> Self {
        self.activation = Some(activation);
        self
    }

    /// Add expiration date
    pub fn with_expiration(mut self, expiration: DateTime<Utc>) -> Self {
        self.expiration = Some(expiration);
        self
    }

    /// Add underlying symbol
    pub fn with_underlying(mut self, underlying: impl Into<String>) -> Self {
        self.underlying = Some(underlying.into());
        self
    }

    /// Add unit of measure
    pub fn with_unit_of_measure(mut self, unit: impl Into<String>) -> Self {
        self.unit_of_measure = Some(unit.into());
        self
    }

    /// Add first notice date
    pub fn with_first_notice_date(mut self, date: NaiveDate) -> Self {
        self.first_notice_date = Some(date);
        self
    }

    /// Add last trading date
    pub fn with_last_trading_date(mut self, date: NaiveDate) -> Self {
        self.last_trading_date = Some(date);
        self
    }

    /// Add roll rule for continuous contracts
    pub fn with_roll_rule(mut self, rule: RollRule) -> Self {
        self.roll_rule = Some(rule);
        self
    }

    /// Check if contract has expired
    pub fn is_expired(&self) -> bool {
        if let Some(exp) = self.expiration {
            Utc::now() > exp
        } else {
            false // Perpetuals never expire
        }
    }

    /// Check if contract is within first notice period
    pub fn is_in_first_notice_period(&self) -> bool {
        if let Some(fnd) = self.first_notice_date {
            Utc::now().date_naive() >= fnd
        } else {
            false
        }
    }

    /// Check if this is a perpetual contract (no expiration)
    pub fn is_perpetual(&self) -> bool {
        self.expiration.is_none()
    }

    /// Check if this is an option contract
    pub fn is_option(&self) -> bool {
        self.strike_price.is_some()
    }

    /// Calculate days to expiration
    pub fn days_to_expiration(&self) -> Option<i64> {
        self.expiration.map(|exp| {
            (exp.date_naive() - Utc::now().date_naive()).num_days()
        })
    }

    /// Calculate notional value
    pub fn notional_value(&self, price: Decimal, quantity: Decimal) -> Decimal {
        price * quantity * self.contract_multiplier
    }

    /// Get contract month code (e.g., "H" for March, "M" for June)
    pub fn month_code(&self) -> Option<char> {
        self.maturity_month.map(|m| month_to_code(m))
    }

    /// Get contract year code (last digit)
    pub fn year_code(&self) -> Option<u8> {
        self.maturity_year.map(|y| (y % 10) as u8)
    }

    /// Generate contract symbol suffix (e.g., "H6" for March 2026)
    pub fn contract_suffix(&self) -> Option<String> {
        match (self.month_code(), self.year_code()) {
            (Some(m), Some(y)) => Some(format!("{}{}", m, y)),
            _ => None,
        }
    }
}

impl Default for ContractSpec {
    fn default() -> Self {
        Self::perpetual(Decimal::ONE)
    }
}

/// Settlement type for derivative contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SettlementType {
    /// Cash settlement (most index futures, crypto)
    #[default]
    Cash,
    /// Physical delivery (commodity futures, some equity options)
    Physical,
}

impl fmt::Display for SettlementType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettlementType::Cash => write!(f, "CASH"),
            SettlementType::Physical => write!(f, "PHYSICAL"),
        }
    }
}

/// Exercise style for options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExerciseStyle {
    /// American - can exercise any time before expiration
    American,
    /// European - can only exercise at expiration
    European,
    /// Bermudan - can exercise on specific dates
    Bermudan,
}

impl fmt::Display for ExerciseStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExerciseStyle::American => write!(f, "AMERICAN"),
            ExerciseStyle::European => write!(f, "EUROPEAN"),
            ExerciseStyle::Bermudan => write!(f, "BERMUDAN"),
        }
    }
}

/// Rules for rolling continuous contracts.
///
/// Aligns with Databento's continuous contract symbology (c, v, n).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollRule {
    /// Roll method matching Databento's continuous notation
    pub roll_method: ContinuousRollMethod,

    /// Rank in the roll chain (0 = front month, 1 = second month, etc.)
    #[serde(default)]
    pub rank: u8,

    /// Days before expiration to roll (for calendar-based rolls)
    #[serde(default)]
    pub roll_days_before: Option<i32>,

    /// Adjustment method for historical prices
    #[serde(default)]
    pub adjustment_method: AdjustmentMethod,
}

impl RollRule {
    /// Create a new roll rule
    pub fn new(roll_method: ContinuousRollMethod) -> Self {
        Self {
            roll_method,
            rank: 0,
            roll_days_before: Some(1),
            adjustment_method: AdjustmentMethod::default(),
        }
    }

    /// Calendar-based roll (front month by expiration)
    pub fn calendar(rank: u8) -> Self {
        Self {
            roll_method: ContinuousRollMethod::Calendar,
            rank,
            roll_days_before: Some(1),
            adjustment_method: AdjustmentMethod::BackAdjust,
        }
    }

    /// Volume-based roll (highest volume)
    pub fn volume(rank: u8) -> Self {
        Self {
            roll_method: ContinuousRollMethod::Volume,
            rank,
            roll_days_before: None,
            adjustment_method: AdjustmentMethod::BackAdjust,
        }
    }

    /// Open interest-based roll
    pub fn open_interest(rank: u8) -> Self {
        Self {
            roll_method: ContinuousRollMethod::OpenInterest,
            rank,
            roll_days_before: None,
            adjustment_method: AdjustmentMethod::BackAdjust,
        }
    }

    /// Set adjustment method
    pub fn with_adjustment(mut self, method: AdjustmentMethod) -> Self {
        self.adjustment_method = method;
        self
    }

    /// Generate Databento-compatible continuous symbol
    /// e.g., "ES", Calendar, 0 -> "ES.c.0"
    pub fn continuous_symbol(&self, base_symbol: &str) -> String {
        format!(
            "{}.{}.{}",
            base_symbol,
            self.roll_method.as_char(),
            self.rank
        )
    }
}

/// Continuous contract roll methods.
///
/// Matches Databento notation: ES.c.0, ES.v.0, ES.n.0
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContinuousRollMethod {
    /// 'c' - Calendar: Roll by expiration date order
    /// Selects contract with nearest expiration
    #[default]
    Calendar,

    /// 'v' - Volume: Roll by highest trading volume
    /// Selects contract with most volume
    Volume,

    /// 'n' - Open Interest: Roll by highest open interest
    /// Selects contract with most open interest
    OpenInterest,
}

impl ContinuousRollMethod {
    /// Get the single character code (Databento format)
    pub fn as_char(&self) -> char {
        match self {
            Self::Calendar => 'c',
            Self::Volume => 'v',
            Self::OpenInterest => 'n',
        }
    }

    /// Parse from single character
    pub fn from_char(c: char) -> Option<Self> {
        match c.to_ascii_lowercase() {
            'c' => Some(Self::Calendar),
            'v' => Some(Self::Volume),
            'n' => Some(Self::OpenInterest),
            _ => None,
        }
    }
}

impl fmt::Display for ContinuousRollMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Calendar => write!(f, "CALENDAR"),
            Self::Volume => write!(f, "VOLUME"),
            Self::OpenInterest => write!(f, "OPEN_INTEREST"),
        }
    }
}

/// Adjustment method for continuous contract prices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdjustmentMethod {
    /// No adjustment (raw prices)
    None,
    /// Ratio adjustment (multiply by factor)
    Ratio,
    /// Difference adjustment (add/subtract)
    Difference,
    /// Back-adjust historical prices (most common)
    #[default]
    BackAdjust,
}

impl fmt::Display for AdjustmentMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "NONE"),
            Self::Ratio => write!(f, "RATIO"),
            Self::Difference => write!(f, "DIFFERENCE"),
            Self::BackAdjust => write!(f, "BACK_ADJUST"),
        }
    }
}

/// Convert month number (1-12) to CME month code.
///
/// CME month codes:
/// F=Jan, G=Feb, H=Mar, J=Apr, K=May, M=Jun,
/// N=Jul, Q=Aug, U=Sep, V=Oct, X=Nov, Z=Dec
pub fn month_to_code(month: u8) -> char {
    match month {
        1 => 'F',
        2 => 'G',
        3 => 'H',
        4 => 'J',
        5 => 'K',
        6 => 'M',
        7 => 'N',
        8 => 'Q',
        9 => 'U',
        10 => 'V',
        11 => 'X',
        12 => 'Z',
        _ => '?',
    }
}

/// Convert CME month code to month number (1-12).
pub fn code_to_month(code: char) -> Option<u8> {
    match code.to_ascii_uppercase() {
        'F' => Some(1),
        'G' => Some(2),
        'H' => Some(3),
        'J' => Some(4),
        'K' => Some(5),
        'M' => Some(6),
        'N' => Some(7),
        'Q' => Some(8),
        'U' => Some(9),
        'V' => Some(10),
        'X' => Some(11),
        'Z' => Some(12),
        _ => None,
    }
}

/// Parse a futures contract symbol to extract base symbol, month, and year.
///
/// Examples:
/// - "ESH6" -> ("ES", 3, 2026)
/// - "CLM25" -> ("CL", 6, 2025)
/// - "GCZ4" -> ("GC", 12, 2024)
pub fn parse_contract_symbol(symbol: &str) -> Option<(String, u8, u16)> {
    if symbol.len() < 3 {
        return None;
    }

    // Find the month code (last letter before digits)
    let chars: Vec<char> = symbol.chars().collect();
    let mut month_idx = None;

    for (i, c) in chars.iter().enumerate().rev() {
        if c.is_ascii_alphabetic() {
            month_idx = Some(i);
            break;
        }
    }

    let month_idx = month_idx?;
    let month_code = chars[month_idx];
    let month = code_to_month(month_code)?;

    // Base symbol is everything before the month code
    let base_symbol: String = chars[..month_idx].iter().collect();

    // Year is the digits after the month code
    let year_str: String = chars[month_idx + 1..].iter().collect();
    let year_suffix: u16 = year_str.parse().ok()?;

    // Convert 1 or 2 digit year to full year
    let year = if year_suffix < 100 {
        // Assume years 00-49 are 2000s, 50-99 are 1900s
        if year_suffix <= 49 {
            2000 + year_suffix
        } else {
            1900 + year_suffix
        }
    } else {
        year_suffix
    };

    Some((base_symbol, month, year))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    #[test]
    fn test_contract_spec_creation() {
        let spec = ContractSpec::future(dec!(50), SettlementType::Cash);
        assert_eq!(spec.contract_multiplier, dec!(50));
        assert_eq!(spec.settlement, SettlementType::Cash);
        assert!(!spec.is_option());
        assert!(spec.is_perpetual()); // No expiration set
    }

    #[test]
    fn test_option_spec() {
        let spec = ContractSpec::option(
            dec!(100),
            dec!(4500),
            OptionType::Call,
            ExerciseStyle::American,
        );

        assert!(spec.is_option());
        assert_eq!(spec.strike_price, Some(dec!(4500)));
        assert_eq!(spec.option_type, Some(OptionType::Call));
        assert_eq!(spec.exercise_style, Some(ExerciseStyle::American));
    }

    #[test]
    fn test_contract_expiration() {
        let future_time = Utc.with_ymd_and_hms(2030, 3, 20, 12, 0, 0).unwrap();
        let spec = ContractSpec::future(dec!(50), SettlementType::Cash)
            .with_expiration(future_time);

        assert!(!spec.is_expired());
        assert!(!spec.is_perpetual());

        let past_time = Utc.with_ymd_and_hms(2020, 3, 20, 12, 0, 0).unwrap();
        let expired_spec = ContractSpec::future(dec!(50), SettlementType::Cash)
            .with_expiration(past_time);

        assert!(expired_spec.is_expired());
    }

    #[test]
    fn test_notional_value() {
        let spec = ContractSpec::future(dec!(50), SettlementType::Cash);
        let notional = spec.notional_value(dec!(4500), dec!(2));
        assert_eq!(notional, dec!(450000)); // 4500 * 2 * 50
    }

    #[test]
    fn test_month_code_conversion() {
        assert_eq!(month_to_code(1), 'F');
        assert_eq!(month_to_code(3), 'H');
        assert_eq!(month_to_code(6), 'M');
        assert_eq!(month_to_code(9), 'U');
        assert_eq!(month_to_code(12), 'Z');

        assert_eq!(code_to_month('H'), Some(3));
        assert_eq!(code_to_month('M'), Some(6));
        assert_eq!(code_to_month('Z'), Some(12));
        assert_eq!(code_to_month('A'), None);
    }

    #[test]
    fn test_parse_contract_symbol() {
        let (base, month, year) = parse_contract_symbol("ESH6").unwrap();
        assert_eq!(base, "ES");
        assert_eq!(month, 3);
        assert_eq!(year, 2006);

        let (base, month, year) = parse_contract_symbol("CLM25").unwrap();
        assert_eq!(base, "CL");
        assert_eq!(month, 6);
        assert_eq!(year, 2025);

        let (base, month, year) = parse_contract_symbol("GCZ24").unwrap();
        assert_eq!(base, "GC");
        assert_eq!(month, 12);
        assert_eq!(year, 2024);
    }

    #[test]
    fn test_roll_method_char() {
        assert_eq!(ContinuousRollMethod::Calendar.as_char(), 'c');
        assert_eq!(ContinuousRollMethod::Volume.as_char(), 'v');
        assert_eq!(ContinuousRollMethod::OpenInterest.as_char(), 'n');

        assert_eq!(ContinuousRollMethod::from_char('c'), Some(ContinuousRollMethod::Calendar));
        assert_eq!(ContinuousRollMethod::from_char('V'), Some(ContinuousRollMethod::Volume));
        assert_eq!(ContinuousRollMethod::from_char('x'), None);
    }

    #[test]
    fn test_continuous_symbol_generation() {
        let rule = RollRule::calendar(0);
        assert_eq!(rule.continuous_symbol("ES"), "ES.c.0");

        let rule = RollRule::volume(1);
        assert_eq!(rule.continuous_symbol("CL"), "CL.v.1");

        let rule = RollRule::open_interest(2);
        assert_eq!(rule.continuous_symbol("GC"), "GC.n.2");
    }

    #[test]
    fn test_contract_suffix() {
        let mut spec = ContractSpec::future(dec!(50), SettlementType::Cash);
        spec.maturity_month = Some(3);
        spec.maturity_year = Some(2026);

        assert_eq!(spec.month_code(), Some('H'));
        assert_eq!(spec.year_code(), Some(6));
        assert_eq!(spec.contract_suffix(), Some("H6".to_string()));
    }
}
