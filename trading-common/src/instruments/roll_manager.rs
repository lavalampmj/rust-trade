//! Roll Manager for continuous contract management.
//!
//! This module provides services for tracking and managing continuous contracts,
//! including roll detection, adjustment factor calculation, and roll event broadcasting.
//!
//! # Features
//!
//! - Continuous contract tracking
//! - Roll trigger detection (calendar, volume, open interest)
//! - Adjustment factor calculation (ratio and difference methods)
//! - Roll event broadcasting
//!
//! # Example
//!
//! ```ignore
//! use trading_common::instruments::RollManager;
//!
//! let manager = RollManager::new(registry.clone());
//!
//! // Register a continuous contract for tracking
//! manager.register_continuous("ES.c.0.GLBX");
//!
//! // Subscribe to roll events
//! let mut rx = manager.subscribe();
//! while let Ok(event) = rx.recv().await {
//!     match event {
//!         RollEvent::RollTriggered { continuous_symbol, from_contract, to_contract } => {
//!             println!("Rolling {} from {} to {}", continuous_symbol, from_contract, to_contract);
//!         }
//!         _ => {}
//!     }
//! }
//! ```

use chrono::{DateTime, Duration, NaiveDate, Utc};
use dashmap::DashMap;
use rust_decimal::Decimal;
use tokio::sync::broadcast;
use tracing::{debug, info};

use super::contract::ContinuousRollMethod;
use super::continuous::{AdjustmentFactor, ContinuousSymbol, RollEvent};
use crate::orders::InstrumentId;

/// Configuration for the roll manager
#[derive(Debug, Clone)]
pub struct RollManagerConfig {
    /// How many days before expiration to start checking for calendar rolls
    pub calendar_roll_days: i64,
    /// Volume threshold ratio for volume-based rolls (e.g., 1.5 means next contract has 50% more volume)
    pub volume_roll_threshold: Decimal,
    /// Open interest threshold ratio for OI-based rolls
    pub oi_roll_threshold: Decimal,
    /// Broadcast channel capacity
    pub channel_capacity: usize,
}

impl Default for RollManagerConfig {
    fn default() -> Self {
        Self {
            calendar_roll_days: 5,
            volume_roll_threshold: Decimal::new(15, 1), // 1.5
            oi_roll_threshold: Decimal::new(15, 1),     // 1.5
            channel_capacity: 256,
        }
    }
}

/// Tracking state for a continuous contract
#[derive(Debug, Clone)]
pub struct ContinuousContractState {
    /// The continuous symbol being tracked
    pub continuous_symbol: ContinuousSymbol,
    /// Current front-month contract ID
    pub current_contract_id: Option<InstrumentId>,
    /// Next contract in the chain
    pub next_contract_id: Option<InstrumentId>,
    /// Expiration date for current contract (used for calendar rolls)
    pub current_expiration: Option<NaiveDate>,
    /// Last known volume for current contract
    pub current_volume: Decimal,
    /// Last known volume for next contract
    pub next_volume: Decimal,
    /// Last known open interest for current contract
    pub current_oi: Decimal,
    /// Last known open interest for next contract
    pub next_oi: Decimal,
    /// Last roll check timestamp
    pub last_check: DateTime<Utc>,
    /// Cumulative adjustment factor
    pub adjustment_factor: Decimal,
}

impl ContinuousContractState {
    /// Create new tracking state for a continuous symbol
    pub fn new(continuous_symbol: ContinuousSymbol) -> Self {
        Self {
            continuous_symbol,
            current_contract_id: None,
            next_contract_id: None,
            current_expiration: None,
            current_volume: Decimal::ZERO,
            next_volume: Decimal::ZERO,
            current_oi: Decimal::ZERO,
            next_oi: Decimal::ZERO,
            last_check: Utc::now(),
            adjustment_factor: Decimal::ONE,
        }
    }
}

/// Roll manager for continuous contracts.
///
/// Tracks continuous contracts and detects when they need to roll
/// from one underlying contract to the next.
pub struct RollManager {
    /// Tracked continuous contracts
    states: DashMap<String, ContinuousContractState>,

    /// Event channel for roll events
    event_tx: broadcast::Sender<RollEvent>,

    /// Configuration
    config: RollManagerConfig,
}

impl RollManager {
    /// Create a new roll manager
    pub fn new() -> Self {
        Self::with_config(RollManagerConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: RollManagerConfig) -> Self {
        let (event_tx, _) = broadcast::channel(config.channel_capacity);

        Self {
            states: DashMap::new(),
            event_tx,
            config,
        }
    }

    /// Subscribe to roll events
    pub fn subscribe(&self) -> broadcast::Receiver<RollEvent> {
        self.event_tx.subscribe()
    }

    /// Register a continuous contract for tracking
    ///
    /// # Arguments
    ///
    /// * `continuous_symbol` - The continuous symbol notation (e.g., "ES.c.0.GLBX")
    pub fn register(&self, continuous_symbol: &str) -> Result<(), RollManagerError> {
        let symbol = ContinuousSymbol::parse(continuous_symbol)
            .map_err(|_| RollManagerError::InvalidSymbol(continuous_symbol.to_string()))?;

        let state = ContinuousContractState::new(symbol);
        self.states.insert(continuous_symbol.to_string(), state);

        debug!("Registered continuous contract for tracking: {}", continuous_symbol);
        Ok(())
    }

    /// Unregister a continuous contract from tracking
    pub fn unregister(&self, continuous_symbol: &str) {
        if self.states.remove(continuous_symbol).is_some() {
            debug!("Unregistered continuous contract: {}", continuous_symbol);
        }
    }

    /// Get the current state for a continuous contract
    pub fn get_state(&self, continuous_symbol: &str) -> Option<ContinuousContractState> {
        self.states.get(continuous_symbol).map(|s| s.clone())
    }

    /// Get the current underlying contract ID for a continuous symbol
    pub fn get_current_contract(&self, continuous_symbol: &str) -> Option<InstrumentId> {
        self.states
            .get(continuous_symbol)
            .and_then(|s| s.current_contract_id.clone())
    }

    /// Update volume data for a contract
    pub fn update_volume(&self, contract_id: &InstrumentId, volume: Decimal) {
        for mut state in self.states.iter_mut() {
            if state.current_contract_id.as_ref() == Some(contract_id) {
                state.current_volume = volume;
            } else if state.next_contract_id.as_ref() == Some(contract_id) {
                state.next_volume = volume;
            }
        }
    }

    /// Update open interest data for a contract
    pub fn update_open_interest(&self, contract_id: &InstrumentId, oi: Decimal) {
        for mut state in self.states.iter_mut() {
            if state.current_contract_id.as_ref() == Some(contract_id) {
                state.current_oi = oi;
            } else if state.next_contract_id.as_ref() == Some(contract_id) {
                state.next_oi = oi;
            }
        }
    }

    /// Check if a roll should occur based on the roll method
    ///
    /// Returns true if a roll should be triggered
    pub fn check_roll(&self, continuous_symbol: &str, now: DateTime<Utc>) -> Option<RollEvent> {
        let state = self.states.get(continuous_symbol)?;

        let should_roll = match state.continuous_symbol.roll_method {
            ContinuousRollMethod::Calendar => {
                self.check_calendar_roll(&state, now)
            }
            ContinuousRollMethod::Volume => {
                self.check_volume_roll(&state)
            }
            ContinuousRollMethod::OpenInterest => {
                self.check_oi_roll(&state)
            }
        };

        if should_roll {
            if let (Some(from), Some(to)) = (&state.current_contract_id, &state.next_contract_id) {
                let event = RollEvent::RollScheduled {
                    continuous_symbol: continuous_symbol.to_string(),
                    from_symbol: from.to_string(),
                    to_symbol: to.to_string(),
                    roll_date: now,
                };

                let _ = self.event_tx.send(event.clone());
                return Some(event);
            }
        }

        None
    }

    /// Check for calendar-based roll
    fn check_calendar_roll(&self, state: &ContinuousContractState, now: DateTime<Utc>) -> bool {
        // Use the stored expiration date from state
        if let Some(expiration) = state.current_expiration {
            // Check if we're within roll_days of expiration
            let roll_threshold = expiration - Duration::days(self.config.calendar_roll_days);
            return now.date_naive() >= roll_threshold;
        }
        false
    }

    /// Check for volume-based roll
    fn check_volume_roll(&self, state: &ContinuousContractState) -> bool {
        if state.current_volume > Decimal::ZERO && state.next_volume > Decimal::ZERO {
            let ratio = state.next_volume / state.current_volume;
            return ratio >= self.config.volume_roll_threshold;
        }
        false
    }

    /// Check for open-interest-based roll
    fn check_oi_roll(&self, state: &ContinuousContractState) -> bool {
        if state.current_oi > Decimal::ZERO && state.next_oi > Decimal::ZERO {
            let ratio = state.next_oi / state.current_oi;
            return ratio >= self.config.oi_roll_threshold;
        }
        false
    }

    /// Execute a roll from current to next contract
    ///
    /// This updates the state and calculates the adjustment factor
    pub fn execute_roll(
        &self,
        continuous_symbol: &str,
        current_price: Decimal,
        next_price: Decimal,
    ) -> Result<Decimal, RollManagerError> {
        let mut state = self
            .states
            .get_mut(continuous_symbol)
            .ok_or(RollManagerError::NotFound(continuous_symbol.to_string()))?;

        let from_contract = state.current_contract_id.clone();
        let to_contract = state.next_contract_id.clone();

        // Calculate adjustment factor (ratio method)
        let adjustment = if current_price > Decimal::ZERO {
            next_price / current_price
        } else {
            Decimal::ONE
        };

        // Update cumulative adjustment factor
        state.adjustment_factor *= adjustment;

        // Shift contracts
        state.current_contract_id = state.next_contract_id.take();
        state.current_expiration = None; // Reset expiration - will be set via set_contracts
        state.current_volume = state.next_volume;
        state.current_oi = state.next_oi;
        state.next_volume = Decimal::ZERO;
        state.next_oi = Decimal::ZERO;
        state.last_check = Utc::now();

        info!(
            "Executed roll for {}: {:?} -> {:?}, adjustment factor: {}",
            continuous_symbol, from_contract, to_contract, adjustment
        );

        // Emit roll executed event
        if let (Some(from), Some(to)) = (from_contract, to_contract) {
            let adjustment_factor = AdjustmentFactor::ratio(
                Utc::now(),
                from.to_string(),
                to.to_string(),
                current_price,
                next_price,
            );
            let event = RollEvent::RollExecuted {
                continuous_symbol: continuous_symbol.to_string(),
                from_symbol: from.to_string(),
                to_symbol: to.to_string(),
                adjustment_factor,
            };
            let _ = self.event_tx.send(event);
        }

        Ok(adjustment)
    }

    /// Set the current and next contracts for a continuous symbol
    ///
    /// The expiration parameter is used for calendar-based roll detection.
    pub fn set_contracts(
        &self,
        continuous_symbol: &str,
        current: InstrumentId,
        next: Option<InstrumentId>,
        expiration: Option<NaiveDate>,
    ) -> Result<(), RollManagerError> {
        let mut state = self
            .states
            .get_mut(continuous_symbol)
            .ok_or(RollManagerError::NotFound(continuous_symbol.to_string()))?;

        state.current_contract_id = Some(current);
        state.next_contract_id = next;
        state.current_expiration = expiration;

        Ok(())
    }

    /// Get the cumulative adjustment factor for a continuous symbol
    pub fn get_adjustment_factor(&self, continuous_symbol: &str) -> Option<Decimal> {
        self.states.get(continuous_symbol).map(|s| s.adjustment_factor)
    }

    /// Apply adjustment factor to a price
    ///
    /// Useful for adjusting historical prices to make them comparable
    pub fn adjust_price(&self, continuous_symbol: &str, raw_price: Decimal) -> Option<Decimal> {
        self.states
            .get(continuous_symbol)
            .map(|s| raw_price * s.adjustment_factor)
    }

    /// Get all tracked continuous symbols
    pub fn list_tracked(&self) -> Vec<String> {
        self.states.iter().map(|e| e.key().clone()).collect()
    }
}

impl Default for RollManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur in the roll manager
#[derive(Debug, Clone, thiserror::Error)]
pub enum RollManagerError {
    #[error("Invalid continuous symbol notation: {0}")]
    InvalidSymbol(String),

    #[error("Continuous contract not found: {0}")]
    NotFound(String),

    #[error("Contract details not available: {0}")]
    ContractNotAvailable(String),

    #[error("Roll failed: {0}")]
    RollFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roll_manager_config_default() {
        let config = RollManagerConfig::default();
        assert_eq!(config.calendar_roll_days, 5);
        assert_eq!(config.volume_roll_threshold, Decimal::new(15, 1));
        assert_eq!(config.oi_roll_threshold, Decimal::new(15, 1));
    }

    #[test]
    fn test_continuous_contract_state_new() {
        let symbol = ContinuousSymbol::parse("ES.c.0").unwrap();
        let state = ContinuousContractState::new(symbol);

        assert!(state.current_contract_id.is_none());
        assert!(state.next_contract_id.is_none());
        assert!(state.current_expiration.is_none());
        assert_eq!(state.adjustment_factor, Decimal::ONE);
    }

    #[test]
    fn test_roll_manager_error_display() {
        let invalid = RollManagerError::InvalidSymbol("invalid".to_string());
        assert!(invalid.to_string().contains("Invalid continuous symbol"));

        let not_found = RollManagerError::NotFound("ES.c.0".to_string());
        assert!(not_found.to_string().contains("not found"));
    }
}
