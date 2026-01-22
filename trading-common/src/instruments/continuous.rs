//! Continuous contract support for futures.
//!
//! This module provides comprehensive support for continuous contracts,
//! following Databento's continuous contract symbology conventions.
//!
//! # Databento Continuous Contract Format
//!
//! Databento uses `{base}.{rule}.{rank}` format:
//! - `ES.c.0` - E-mini S&P 500, calendar-based, front month
//! - `CL.v.0` - Crude Oil, volume-based, highest volume
//! - `GC.n.1` - Gold, open-interest-based, second-highest OI
//!
//! # Roll Methods
//!
//! - `c` (Calendar): Roll by expiration date order
//! - `v` (Volume): Roll to highest volume contract
//! - `n` (Open Interest): Roll to highest open interest contract
//!
//! # Example
//!
//! ```ignore
//! use trading_common::instruments::continuous::{ContinuousContract, ContinuousSymbol};
//!
//! // Parse a continuous symbol
//! let symbol = ContinuousSymbol::parse("ES.c.0")?;
//! assert_eq!(symbol.base_symbol, "ES");
//! assert_eq!(symbol.roll_method, ContinuousRollMethod::Calendar);
//! assert_eq!(symbol.rank, 0);
//!
//! // Create a continuous contract
//! let contract = ContinuousContract::new("ES", "GLBX", ContinuousRollMethod::Calendar, 0);
//! ```

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::orders::InstrumentId;
use super::contract::ContinuousRollMethod;

/// Parsed continuous contract symbol.
///
/// Represents a Databento-format continuous symbol: `{base}.{rule}.{rank}` or `{base}.{rule}.{rank}.{venue}`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContinuousSymbol {
    /// Base/asset symbol (e.g., "ES", "CL", "GC")
    pub base_symbol: String,

    /// Roll method ('c' = calendar, 'v' = volume, 'n' = open interest)
    pub roll_method: ContinuousRollMethod,

    /// Rank in the roll chain (0 = front, 1 = second, etc.)
    pub rank: u8,

    /// Optional venue (e.g., "GLBX" for CME Globex)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,
}

impl ContinuousSymbol {
    /// Create a new continuous symbol without venue
    pub fn new(base: impl Into<String>, roll_method: ContinuousRollMethod, rank: u8) -> Self {
        Self {
            base_symbol: base.into(),
            roll_method,
            rank,
            venue: None,
        }
    }

    /// Create a new continuous symbol with venue
    pub fn with_venue(
        base: impl Into<String>,
        roll_method: ContinuousRollMethod,
        rank: u8,
        venue: impl Into<String>,
    ) -> Self {
        Self {
            base_symbol: base.into(),
            roll_method,
            rank,
            venue: Some(venue.into()),
        }
    }

    /// Parse a Databento continuous symbol string.
    ///
    /// Format: `{base}.{rule}.{rank}` or `{base}.{rule}.{rank}.{venue}`
    ///
    /// # Examples
    /// - "ES.c.0" -> base="ES", method=Calendar, rank=0, venue=None
    /// - "ES.c.0.GLBX" -> base="ES", method=Calendar, rank=0, venue=Some("GLBX")
    /// - "CL.v.1" -> base="CL", method=Volume, rank=1, venue=None
    /// - "GC.n.2.COMX" -> base="GC", method=OpenInterest, rank=2, venue=Some("COMX")
    pub fn parse(symbol: &str) -> Result<Self, ContinuousSymbolError> {
        let parts: Vec<&str> = symbol.split('.').collect();

        if parts.len() != 3 && parts.len() != 4 {
            return Err(ContinuousSymbolError::InvalidFormat(format!(
                "Expected 'base.method.rank' or 'base.method.rank.venue' format, got '{}'",
                symbol
            )));
        }

        let base_symbol = parts[0].to_string();
        if base_symbol.is_empty() {
            return Err(ContinuousSymbolError::InvalidFormat(
                "Base symbol cannot be empty".to_string(),
            ));
        }

        let roll_method = ContinuousRollMethod::from_char(parts[1].chars().next().unwrap_or('?'))
            .ok_or_else(|| {
                ContinuousSymbolError::InvalidRollMethod(format!(
                    "Invalid roll method '{}', expected 'c', 'v', or 'n'",
                    parts[1]
                ))
            })?;

        let rank = parts[2].parse::<u8>().map_err(|_| {
            ContinuousSymbolError::InvalidRank(format!(
                "Invalid rank '{}', expected a non-negative integer",
                parts[2]
            ))
        })?;

        let venue = if parts.len() == 4 {
            Some(parts[3].to_string())
        } else {
            None
        };

        Ok(Self {
            base_symbol,
            roll_method,
            rank,
            venue,
        })
    }

    /// Check if this is a continuous symbol string
    pub fn is_continuous(symbol: &str) -> bool {
        let parts: Vec<&str> = symbol.split('.').collect();
        if parts.len() != 3 && parts.len() != 4 {
            return false;
        }
        // Check if second part is a valid roll method and third is a valid rank
        ContinuousRollMethod::from_char(parts[1].chars().next().unwrap_or('?')).is_some()
            && parts[2].parse::<u8>().is_ok()
    }

    /// Convert to string format (without venue)
    pub fn to_symbol_string(&self) -> String {
        format!("{}.{}.{}", self.base_symbol, self.roll_method.as_char(), self.rank)
    }

    /// Convert to full string format (with venue if present)
    pub fn to_full_string(&self) -> String {
        if let Some(ref venue) = self.venue {
            format!("{}.{}.{}.{}", self.base_symbol, self.roll_method.as_char(), self.rank, venue)
        } else {
            self.to_symbol_string()
        }
    }

    /// Create front month calendar-based symbol
    pub fn front_month(base: impl Into<String>) -> Self {
        Self::new(base, ContinuousRollMethod::Calendar, 0)
    }

    /// Create front month volume-based symbol
    pub fn front_volume(base: impl Into<String>) -> Self {
        Self::new(base, ContinuousRollMethod::Volume, 0)
    }

    /// Create front month open-interest-based symbol
    pub fn front_open_interest(base: impl Into<String>) -> Self {
        Self::new(base, ContinuousRollMethod::OpenInterest, 0)
    }
}

impl fmt::Display for ContinuousSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_full_string())
    }
}

impl std::str::FromStr for ContinuousSymbol {
    type Err = ContinuousSymbolError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Error type for continuous symbol parsing.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ContinuousSymbolError {
    /// Invalid format
    #[error("Invalid continuous symbol format: {0}")]
    InvalidFormat(String),

    /// Invalid roll method
    #[error("Invalid roll method: {0}")]
    InvalidRollMethod(String),

    /// Invalid rank
    #[error("Invalid rank: {0}")]
    InvalidRank(String),
}

/// Continuous contract that maps to underlying futures.
///
/// Uses Databento notation: `{base}.{rule}.{rank}`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuousContract {
    /// Continuous contract symbol specification
    pub symbol: ContinuousSymbol,

    /// Venue (e.g., "GLBX" for CME Globex)
    pub venue: String,

    /// Full identifier including venue
    pub id: InstrumentId,

    /// Currently mapped underlying contract
    #[serde(default)]
    pub current_contract: Option<InstrumentId>,

    /// Current contract's raw symbol (e.g., "ESH6")
    #[serde(default)]
    pub current_raw_symbol: Option<String>,

    /// Contract chain (ordered by rank)
    #[serde(default)]
    pub contract_chain: Vec<ContractInfo>,

    /// Adjustment factors for back-adjusted prices
    #[serde(default)]
    pub adjustment_factors: Vec<AdjustmentFactor>,

    /// Last time the mapping was updated
    #[serde(default = "Utc::now")]
    pub last_updated: DateTime<Utc>,
}

impl ContinuousContract {
    /// Create a new continuous contract
    pub fn new(
        base: impl Into<String>,
        venue: impl Into<String>,
        roll_method: ContinuousRollMethod,
        rank: u8,
    ) -> Self {
        let base = base.into();
        let venue = venue.into();
        let symbol = ContinuousSymbol::new(&base, roll_method, rank);
        let id = InstrumentId::new(symbol.to_symbol_string(), &venue);

        Self {
            symbol,
            venue,
            id,
            current_contract: None,
            current_raw_symbol: None,
            contract_chain: Vec::new(),
            adjustment_factors: Vec::new(),
            last_updated: Utc::now(),
        }
    }

    /// Create from a parsed continuous symbol
    pub fn from_symbol(symbol: ContinuousSymbol, venue: impl Into<String>) -> Self {
        let venue = venue.into();
        let id = InstrumentId::new(symbol.to_symbol_string(), &venue);

        Self {
            symbol,
            venue,
            id,
            current_contract: None,
            current_raw_symbol: None,
            contract_chain: Vec::new(),
            adjustment_factors: Vec::new(),
            last_updated: Utc::now(),
        }
    }

    /// Get the continuous symbol string
    pub fn continuous_symbol(&self) -> String {
        self.symbol.to_symbol_string()
    }

    /// Get the base symbol
    pub fn base_symbol(&self) -> &str {
        &self.symbol.base_symbol
    }

    /// Get the roll method
    pub fn roll_method(&self) -> ContinuousRollMethod {
        self.symbol.roll_method
    }

    /// Get the rank
    pub fn rank(&self) -> u8 {
        self.symbol.rank
    }

    /// Update the current contract mapping
    pub fn update_mapping(
        &mut self,
        contract_id: InstrumentId,
        raw_symbol: String,
    ) {
        self.current_contract = Some(contract_id);
        self.current_raw_symbol = Some(raw_symbol);
        self.last_updated = Utc::now();
    }

    /// Update the contract chain
    pub fn update_chain(&mut self, chain: Vec<ContractInfo>) {
        self.contract_chain = chain;
        self.last_updated = Utc::now();
    }

    /// Add an adjustment factor
    pub fn add_adjustment(&mut self, factor: AdjustmentFactor) {
        self.adjustment_factors.push(factor);
    }

    /// Get the contract at a specific rank (0 = front, 1 = second, etc.)
    pub fn get_contract_at_rank(&self, rank: usize) -> Option<&ContractInfo> {
        self.contract_chain.get(rank)
    }

    /// Get the current front month contract
    pub fn front_contract(&self) -> Option<&ContractInfo> {
        self.get_contract_at_rank(self.symbol.rank as usize)
    }

    /// Check if a roll is needed based on the roll method
    pub fn needs_roll(&self) -> bool {
        // Simple check - actual logic would depend on roll method
        // For calendar: check if near expiration
        // For volume/OI: check if another contract has higher volume/OI
        if self.contract_chain.is_empty() {
            return false;
        }

        match self.symbol.roll_method {
            ContinuousRollMethod::Calendar => {
                // Check if front contract is near expiration
                if let Some(front) = self.front_contract() {
                    if let Some(exp) = front.expiration {
                        let days_to_exp = (exp - Utc::now()).num_days();
                        days_to_exp <= 1
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            ContinuousRollMethod::Volume => {
                // Check if next contract has higher volume
                if let (Some(front), Some(next)) = (
                    self.get_contract_at_rank(self.symbol.rank as usize),
                    self.get_contract_at_rank(self.symbol.rank as usize + 1),
                ) {
                    next.volume.unwrap_or(0) > front.volume.unwrap_or(0)
                } else {
                    false
                }
            }
            ContinuousRollMethod::OpenInterest => {
                // Check if next contract has higher OI
                if let (Some(front), Some(next)) = (
                    self.get_contract_at_rank(self.symbol.rank as usize),
                    self.get_contract_at_rank(self.symbol.rank as usize + 1),
                ) {
                    next.open_interest.unwrap_or(0) > front.open_interest.unwrap_or(0)
                } else {
                    false
                }
            }
        }
    }

    /// Adjust a historical price using accumulated adjustment factors
    pub fn adjust_price(&self, price: Decimal, as_of: DateTime<Utc>) -> Decimal {
        let mut adjusted = price;

        for factor in &self.adjustment_factors {
            if as_of < factor.roll_date {
                match factor.adjustment_type {
                    AdjustmentType::Ratio => adjusted *= factor.ratio,
                    AdjustmentType::Difference => adjusted += factor.difference,
                }
            }
        }

        adjusted
    }
}

impl fmt::Display for ContinuousContract {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.continuous_symbol(), self.venue)
    }
}

/// Information about a specific contract in the chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractInfo {
    /// Raw symbol (e.g., "ESH6", "ESM6")
    pub raw_symbol: String,

    /// Numeric instrument ID (from exchange/provider)
    #[serde(default)]
    pub instrument_id: Option<u64>,

    /// Full identifier
    pub id: InstrumentId,

    /// Expiration date
    #[serde(default)]
    pub expiration: Option<DateTime<Utc>>,

    /// First notice date (if applicable)
    #[serde(default)]
    pub first_notice: Option<chrono::NaiveDate>,

    /// Last trading date
    #[serde(default)]
    pub last_trading: Option<chrono::NaiveDate>,

    /// Current open interest
    #[serde(default)]
    pub open_interest: Option<u64>,

    /// Current daily volume
    #[serde(default)]
    pub volume: Option<u64>,

    /// Last trade price
    #[serde(default)]
    pub last_price: Option<Decimal>,

    /// Settlement price
    #[serde(default)]
    pub settlement_price: Option<Decimal>,

    /// Last update timestamp
    #[serde(default = "Utc::now")]
    pub last_updated: DateTime<Utc>,
}

impl ContractInfo {
    /// Create a new contract info
    pub fn new(raw_symbol: impl Into<String>, venue: impl Into<String>) -> Self {
        let raw_symbol = raw_symbol.into();
        let venue = venue.into();
        let id = InstrumentId::new(&raw_symbol, &venue);

        Self {
            raw_symbol,
            instrument_id: None,
            id,
            expiration: None,
            first_notice: None,
            last_trading: None,
            open_interest: None,
            volume: None,
            last_price: None,
            settlement_price: None,
            last_updated: Utc::now(),
        }
    }

    /// Add expiration date
    pub fn with_expiration(mut self, expiration: DateTime<Utc>) -> Self {
        self.expiration = Some(expiration);
        self
    }

    /// Add volume
    pub fn with_volume(mut self, volume: u64) -> Self {
        self.volume = Some(volume);
        self
    }

    /// Add open interest
    pub fn with_open_interest(mut self, oi: u64) -> Self {
        self.open_interest = Some(oi);
        self
    }

    /// Check if this contract is expired
    pub fn is_expired(&self) -> bool {
        self.expiration.map(|e| Utc::now() > e).unwrap_or(false)
    }

    /// Days to expiration (negative if expired)
    pub fn days_to_expiration(&self) -> Option<i64> {
        self.expiration.map(|e| {
            (e.date_naive() - Utc::now().date_naive()).num_days()
        })
    }
}

/// Adjustment factor for a single roll event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdjustmentFactor {
    /// Date when the roll occurred
    pub roll_date: DateTime<Utc>,

    /// Symbol rolled from
    pub from_symbol: String,

    /// Symbol rolled to
    pub to_symbol: String,

    /// Type of adjustment
    pub adjustment_type: AdjustmentType,

    /// Ratio adjustment factor: new_price = old_price * ratio
    pub ratio: Decimal,

    /// Difference adjustment: new_price = old_price + difference
    pub difference: Decimal,

    /// Settlement price of old contract at roll
    pub old_price: Decimal,

    /// Settlement price of new contract at roll
    pub new_price: Decimal,
}

impl AdjustmentFactor {
    /// Create a ratio-based adjustment
    pub fn ratio(
        roll_date: DateTime<Utc>,
        from_symbol: impl Into<String>,
        to_symbol: impl Into<String>,
        old_price: Decimal,
        new_price: Decimal,
    ) -> Self {
        let ratio = if old_price != Decimal::ZERO {
            new_price / old_price
        } else {
            Decimal::ONE
        };

        Self {
            roll_date,
            from_symbol: from_symbol.into(),
            to_symbol: to_symbol.into(),
            adjustment_type: AdjustmentType::Ratio,
            ratio,
            difference: Decimal::ZERO,
            old_price,
            new_price,
        }
    }

    /// Create a difference-based adjustment
    pub fn difference(
        roll_date: DateTime<Utc>,
        from_symbol: impl Into<String>,
        to_symbol: impl Into<String>,
        old_price: Decimal,
        new_price: Decimal,
    ) -> Self {
        let difference = new_price - old_price;

        Self {
            roll_date,
            from_symbol: from_symbol.into(),
            to_symbol: to_symbol.into(),
            adjustment_type: AdjustmentType::Difference,
            ratio: Decimal::ONE,
            difference,
            old_price,
            new_price,
        }
    }
}

/// Type of price adjustment for roll gaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdjustmentType {
    /// Multiply historical prices by ratio
    Ratio,
    /// Add/subtract difference to historical prices
    #[default]
    Difference,
}

impl fmt::Display for AdjustmentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdjustmentType::Ratio => write!(f, "RATIO"),
            AdjustmentType::Difference => write!(f, "DIFFERENCE"),
        }
    }
}

/// Roll event for continuous contracts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RollEvent {
    /// Roll is scheduled to occur
    RollScheduled {
        continuous_symbol: String,
        from_symbol: String,
        to_symbol: String,
        roll_date: DateTime<Utc>,
    },
    /// Roll has been executed
    RollExecuted {
        continuous_symbol: String,
        from_symbol: String,
        to_symbol: String,
        adjustment_factor: AdjustmentFactor,
    },
    /// Roll was cancelled
    RollCancelled {
        continuous_symbol: String,
        reason: String,
    },
    /// Mapping changed (e.g., due to volume/OI shift)
    MappingChanged {
        continuous_symbol: String,
        old_symbol: String,
        new_symbol: String,
    },
}

impl fmt::Display for RollEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RollEvent::RollScheduled {
                continuous_symbol,
                from_symbol,
                to_symbol,
                roll_date,
            } => write!(
                f,
                "Roll scheduled: {} {} -> {} at {}",
                continuous_symbol, from_symbol, to_symbol, roll_date
            ),
            RollEvent::RollExecuted {
                continuous_symbol,
                from_symbol,
                to_symbol,
                ..
            } => write!(
                f,
                "Roll executed: {} {} -> {}",
                continuous_symbol, from_symbol, to_symbol
            ),
            RollEvent::RollCancelled {
                continuous_symbol,
                reason,
            } => write!(f, "Roll cancelled: {} - {}", continuous_symbol, reason),
            RollEvent::MappingChanged {
                continuous_symbol,
                old_symbol,
                new_symbol,
            } => write!(
                f,
                "Mapping changed: {} {} -> {}",
                continuous_symbol, old_symbol, new_symbol
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    #[test]
    fn test_continuous_symbol_parse() {
        let symbol = ContinuousSymbol::parse("ES.c.0").unwrap();
        assert_eq!(symbol.base_symbol, "ES");
        assert_eq!(symbol.roll_method, ContinuousRollMethod::Calendar);
        assert_eq!(symbol.rank, 0);

        let symbol = ContinuousSymbol::parse("CL.v.1").unwrap();
        assert_eq!(symbol.base_symbol, "CL");
        assert_eq!(symbol.roll_method, ContinuousRollMethod::Volume);
        assert_eq!(symbol.rank, 1);

        let symbol = ContinuousSymbol::parse("GC.n.2").unwrap();
        assert_eq!(symbol.base_symbol, "GC");
        assert_eq!(symbol.roll_method, ContinuousRollMethod::OpenInterest);
        assert_eq!(symbol.rank, 2);
    }

    #[test]
    fn test_continuous_symbol_invalid() {
        assert!(ContinuousSymbol::parse("ES").is_err());
        assert!(ContinuousSymbol::parse("ES.c").is_err());
        assert!(ContinuousSymbol::parse("ES.x.0").is_err());
        assert!(ContinuousSymbol::parse("ES.c.abc").is_err());
        assert!(ContinuousSymbol::parse(".c.0").is_err());
    }

    #[test]
    fn test_is_continuous() {
        assert!(ContinuousSymbol::is_continuous("ES.c.0"));
        assert!(ContinuousSymbol::is_continuous("CL.v.1"));
        assert!(ContinuousSymbol::is_continuous("GC.n.2"));

        assert!(!ContinuousSymbol::is_continuous("ES"));
        assert!(!ContinuousSymbol::is_continuous("ESH6"));
        assert!(!ContinuousSymbol::is_continuous("ES.GLBX"));
        assert!(!ContinuousSymbol::is_continuous("ES.x.0"));
    }

    #[test]
    fn test_continuous_symbol_to_string() {
        let symbol = ContinuousSymbol::new("ES", ContinuousRollMethod::Calendar, 0);
        assert_eq!(symbol.to_symbol_string(), "ES.c.0");
        assert_eq!(format!("{}", symbol), "ES.c.0");
    }

    #[test]
    fn test_continuous_contract_creation() {
        let contract = ContinuousContract::new("ES", "GLBX", ContinuousRollMethod::Calendar, 0);
        assert_eq!(contract.continuous_symbol(), "ES.c.0");
        assert_eq!(contract.venue, "GLBX");
        assert_eq!(contract.base_symbol(), "ES");
        assert_eq!(contract.rank(), 0);
    }

    #[test]
    fn test_contract_info() {
        let exp = Utc.with_ymd_and_hms(2030, 3, 20, 12, 0, 0).unwrap();
        let info = ContractInfo::new("ESH30", "GLBX")
            .with_expiration(exp)
            .with_volume(100000)
            .with_open_interest(500000);

        assert_eq!(info.raw_symbol, "ESH30");
        assert_eq!(info.volume, Some(100000));
        assert_eq!(info.open_interest, Some(500000));
        assert!(!info.is_expired());
    }

    #[test]
    fn test_adjustment_factor_ratio() {
        let roll_date = Utc::now();
        let factor = AdjustmentFactor::ratio(roll_date, "ESH6", "ESM6", dec!(4500), dec!(4510));

        assert_eq!(factor.adjustment_type, AdjustmentType::Ratio);
        // Ratio should be 4510/4500
        assert!(factor.ratio > Decimal::ONE);
    }

    #[test]
    fn test_adjustment_factor_difference() {
        let roll_date = Utc::now();
        let factor = AdjustmentFactor::difference(roll_date, "ESH6", "ESM6", dec!(4500), dec!(4510));

        assert_eq!(factor.adjustment_type, AdjustmentType::Difference);
        assert_eq!(factor.difference, dec!(10));
    }

    #[test]
    fn test_price_adjustment() {
        let mut contract = ContinuousContract::new("ES", "GLBX", ContinuousRollMethod::Calendar, 0);

        let roll_date = Utc.with_ymd_and_hms(2024, 3, 15, 12, 0, 0).unwrap();
        let factor = AdjustmentFactor::difference(roll_date, "ESH4", "ESM4", dec!(5000), dec!(5010));
        contract.add_adjustment(factor);

        // Price from before the roll should be adjusted
        let old_date = Utc.with_ymd_and_hms(2024, 3, 10, 12, 0, 0).unwrap();
        let adjusted = contract.adjust_price(dec!(4990), old_date);
        assert_eq!(adjusted, dec!(5000)); // 4990 + 10

        // Price from after the roll should not be adjusted
        let new_date = Utc.with_ymd_and_hms(2024, 3, 20, 12, 0, 0).unwrap();
        let adjusted = contract.adjust_price(dec!(5020), new_date);
        assert_eq!(adjusted, dec!(5020)); // unchanged
    }

    #[test]
    fn test_front_month_helpers() {
        let cal = ContinuousSymbol::front_month("ES");
        assert_eq!(cal.roll_method, ContinuousRollMethod::Calendar);
        assert_eq!(cal.rank, 0);

        let vol = ContinuousSymbol::front_volume("CL");
        assert_eq!(vol.roll_method, ContinuousRollMethod::Volume);

        let oi = ContinuousSymbol::front_open_interest("GC");
        assert_eq!(oi.roll_method, ContinuousRollMethod::OpenInterest);
    }
}
