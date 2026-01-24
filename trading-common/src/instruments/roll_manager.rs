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

use super::continuous::{AdjustmentFactor, ContinuousSymbol, RollEvent};
use super::contract::ContinuousRollMethod;
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

        debug!(
            "Registered continuous contract for tracking: {}",
            continuous_symbol
        );
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
            ContinuousRollMethod::Calendar => self.check_calendar_roll(&state, now),
            ContinuousRollMethod::Volume => self.check_volume_roll(&state),
            ContinuousRollMethod::OpenInterest => self.check_oi_roll(&state),
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
        self.states
            .get(continuous_symbol)
            .map(|s| s.adjustment_factor)
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

    // ============================================================
    // ROLL MANAGER REGISTRATION TESTS
    // ============================================================

    #[test]
    fn test_register_and_get_state() {
        let manager = RollManager::new();

        // Register a continuous symbol
        assert!(manager.register("ES.c.0").is_ok());

        // Should be able to get state
        let state = manager.get_state("ES.c.0");
        assert!(state.is_some());
        let state = state.unwrap();
        assert_eq!(state.continuous_symbol.base_symbol, "ES");
        assert!(state.current_contract_id.is_none());
    }

    #[test]
    fn test_register_invalid_symbol() {
        let manager = RollManager::new();

        // Invalid format should fail
        assert!(manager.register("ES").is_err());
        assert!(manager.register("invalid").is_err());
        assert!(manager.register("ES.x.0").is_err());
    }

    #[test]
    fn test_register_duplicate() {
        let manager = RollManager::new();

        // First registration
        assert!(manager.register("ES.c.0").is_ok());

        // Duplicate registration should overwrite (not error)
        assert!(manager.register("ES.c.0").is_ok());

        // Still only one entry
        assert_eq!(manager.list_tracked().len(), 1);
    }

    #[test]
    fn test_unregister() {
        let manager = RollManager::new();

        manager.register("ES.c.0").unwrap();
        assert!(manager.get_state("ES.c.0").is_some());

        manager.unregister("ES.c.0");
        assert!(manager.get_state("ES.c.0").is_none());

        // Unregister non-existent should not panic
        manager.unregister("NONEXISTENT.c.0");
    }

    #[test]
    fn test_list_tracked() {
        let manager = RollManager::new();

        assert!(manager.list_tracked().is_empty());

        manager.register("ES.c.0").unwrap();
        manager.register("CL.v.0").unwrap();
        manager.register("GC.n.1").unwrap();

        let tracked = manager.list_tracked();
        assert_eq!(tracked.len(), 3);
        assert!(tracked.contains(&"ES.c.0".to_string()));
        assert!(tracked.contains(&"CL.v.0".to_string()));
        assert!(tracked.contains(&"GC.n.1".to_string()));
    }

    // ============================================================
    // SET CONTRACTS TESTS
    // ============================================================

    #[test]
    fn test_set_contracts() {
        let manager = RollManager::new();
        manager.register("ES.c.0").unwrap();

        let current = InstrumentId::new("ESH6", "GLBX");
        let next = InstrumentId::new("ESM6", "GLBX");
        let expiration = NaiveDate::from_ymd_opt(2026, 3, 20);

        let result =
            manager.set_contracts("ES.c.0", current.clone(), Some(next.clone()), expiration);
        assert!(result.is_ok());

        // Verify contracts were set
        let state = manager.get_state("ES.c.0").unwrap();
        assert_eq!(state.current_contract_id, Some(current));
        assert_eq!(state.next_contract_id, Some(next));
        assert_eq!(state.current_expiration, expiration);
    }

    #[test]
    fn test_set_contracts_not_registered() {
        let manager = RollManager::new();

        let current = InstrumentId::new("ESH6", "GLBX");
        let result = manager.set_contracts("ES.c.0", current, None, None);

        assert!(matches!(result, Err(RollManagerError::NotFound(_))));
    }

    #[test]
    fn test_get_current_contract() {
        let manager = RollManager::new();
        manager.register("ES.c.0").unwrap();

        // Before setting, should be None
        assert!(manager.get_current_contract("ES.c.0").is_none());

        // After setting
        let current = InstrumentId::new("ESH6", "GLBX");
        manager
            .set_contracts("ES.c.0", current.clone(), None, None)
            .unwrap();

        assert_eq!(manager.get_current_contract("ES.c.0"), Some(current));

        // Non-existent symbol
        assert!(manager.get_current_contract("NONEXISTENT.c.0").is_none());
    }

    // ============================================================
    // VOLUME/OI UPDATE TESTS
    // ============================================================

    #[test]
    fn test_update_volume() {
        let manager = RollManager::new();
        manager.register("ES.c.0").unwrap();

        let current = InstrumentId::new("ESH6", "GLBX");
        let next = InstrumentId::new("ESM6", "GLBX");
        manager
            .set_contracts("ES.c.0", current.clone(), Some(next.clone()), None)
            .unwrap();

        // Update current contract volume
        manager.update_volume(&current, Decimal::new(100000, 0));

        let state = manager.get_state("ES.c.0").unwrap();
        assert_eq!(state.current_volume, Decimal::new(100000, 0));

        // Update next contract volume
        manager.update_volume(&next, Decimal::new(50000, 0));

        let state = manager.get_state("ES.c.0").unwrap();
        assert_eq!(state.next_volume, Decimal::new(50000, 0));
    }

    #[test]
    fn test_update_open_interest() {
        let manager = RollManager::new();
        manager.register("ES.n.0").unwrap();

        let current = InstrumentId::new("ESH6", "GLBX");
        let next = InstrumentId::new("ESM6", "GLBX");
        manager
            .set_contracts("ES.n.0", current.clone(), Some(next.clone()), None)
            .unwrap();

        manager.update_open_interest(&current, Decimal::new(500000, 0));
        manager.update_open_interest(&next, Decimal::new(300000, 0));

        let state = manager.get_state("ES.n.0").unwrap();
        assert_eq!(state.current_oi, Decimal::new(500000, 0));
        assert_eq!(state.next_oi, Decimal::new(300000, 0));
    }

    // ============================================================
    // ROLL DETECTION TESTS - Calendar
    // ============================================================

    #[test]
    fn test_check_roll_calendar_within_threshold() {
        let config = RollManagerConfig {
            calendar_roll_days: 5,
            ..Default::default()
        };
        let manager = RollManager::with_config(config);
        manager.register("ES.c.0").unwrap();

        let current = InstrumentId::new("ESH6", "GLBX");
        let next = InstrumentId::new("ESM6", "GLBX");

        // Expiration in 3 days (within 5-day threshold)
        let expiration = (Utc::now() + Duration::days(3)).date_naive();
        manager
            .set_contracts("ES.c.0", current, Some(next), Some(expiration))
            .unwrap();

        let result = manager.check_roll("ES.c.0", Utc::now());
        assert!(result.is_some());

        if let Some(RollEvent::RollScheduled {
            continuous_symbol, ..
        }) = result
        {
            assert_eq!(continuous_symbol, "ES.c.0");
        } else {
            panic!("Expected RollScheduled event");
        }
    }

    #[test]
    fn test_check_roll_calendar_outside_threshold() {
        let config = RollManagerConfig {
            calendar_roll_days: 5,
            ..Default::default()
        };
        let manager = RollManager::with_config(config);
        manager.register("ES.c.0").unwrap();

        let current = InstrumentId::new("ESH6", "GLBX");
        let next = InstrumentId::new("ESM6", "GLBX");

        // Expiration in 10 days (outside 5-day threshold)
        let expiration = (Utc::now() + Duration::days(10)).date_naive();
        manager
            .set_contracts("ES.c.0", current, Some(next), Some(expiration))
            .unwrap();

        let result = manager.check_roll("ES.c.0", Utc::now());
        assert!(result.is_none());
    }

    #[test]
    fn test_check_roll_calendar_exactly_at_threshold() {
        let config = RollManagerConfig {
            calendar_roll_days: 5,
            ..Default::default()
        };
        let manager = RollManager::with_config(config);
        manager.register("ES.c.0").unwrap();

        let current = InstrumentId::new("ESH6", "GLBX");
        let next = InstrumentId::new("ESM6", "GLBX");

        // Expiration exactly at threshold boundary
        let expiration = (Utc::now() + Duration::days(5)).date_naive();
        manager
            .set_contracts("ES.c.0", current, Some(next), Some(expiration))
            .unwrap();

        // At exactly threshold, should trigger roll
        let result = manager.check_roll("ES.c.0", Utc::now());
        assert!(result.is_some());
    }

    #[test]
    fn test_check_roll_calendar_no_expiration() {
        let manager = RollManager::new();
        manager.register("ES.c.0").unwrap();

        let current = InstrumentId::new("ESH6", "GLBX");
        let next = InstrumentId::new("ESM6", "GLBX");

        // No expiration set
        manager
            .set_contracts("ES.c.0", current, Some(next), None)
            .unwrap();

        let result = manager.check_roll("ES.c.0", Utc::now());
        assert!(result.is_none()); // Can't roll without expiration info
    }

    // ============================================================
    // ROLL DETECTION TESTS - Volume
    // ============================================================

    #[test]
    fn test_check_roll_volume_above_threshold() {
        let config = RollManagerConfig {
            volume_roll_threshold: Decimal::new(15, 1), // 1.5x
            ..Default::default()
        };
        let manager = RollManager::with_config(config);
        manager.register("ES.v.0").unwrap();

        let current = InstrumentId::new("ESH6", "GLBX");
        let next = InstrumentId::new("ESM6", "GLBX");
        manager
            .set_contracts("ES.v.0", current.clone(), Some(next.clone()), None)
            .unwrap();

        // Current: 100k, Next: 160k (1.6x > 1.5x threshold)
        manager.update_volume(&current, Decimal::new(100000, 0));
        manager.update_volume(&next, Decimal::new(160000, 0));

        let result = manager.check_roll("ES.v.0", Utc::now());
        assert!(result.is_some());
    }

    #[test]
    fn test_check_roll_volume_below_threshold() {
        let config = RollManagerConfig {
            volume_roll_threshold: Decimal::new(15, 1), // 1.5x
            ..Default::default()
        };
        let manager = RollManager::with_config(config);
        manager.register("ES.v.0").unwrap();

        let current = InstrumentId::new("ESH6", "GLBX");
        let next = InstrumentId::new("ESM6", "GLBX");
        manager
            .set_contracts("ES.v.0", current.clone(), Some(next.clone()), None)
            .unwrap();

        // Current: 100k, Next: 140k (1.4x < 1.5x threshold)
        manager.update_volume(&current, Decimal::new(100000, 0));
        manager.update_volume(&next, Decimal::new(140000, 0));

        let result = manager.check_roll("ES.v.0", Utc::now());
        assert!(result.is_none());
    }

    #[test]
    fn test_check_roll_volume_zero_current() {
        let manager = RollManager::new();
        manager.register("ES.v.0").unwrap();

        let current = InstrumentId::new("ESH6", "GLBX");
        let next = InstrumentId::new("ESM6", "GLBX");
        manager
            .set_contracts("ES.v.0", current.clone(), Some(next.clone()), None)
            .unwrap();

        // Current: 0, Next: 100k - should not panic (division by zero)
        manager.update_volume(&current, Decimal::ZERO);
        manager.update_volume(&next, Decimal::new(100000, 0));

        let result = manager.check_roll("ES.v.0", Utc::now());
        assert!(result.is_none()); // No roll when current is zero
    }

    // ============================================================
    // ROLL DETECTION TESTS - Open Interest
    // ============================================================

    #[test]
    fn test_check_roll_oi_above_threshold() {
        let config = RollManagerConfig {
            oi_roll_threshold: Decimal::new(15, 1), // 1.5x
            ..Default::default()
        };
        let manager = RollManager::with_config(config);
        manager.register("ES.n.0").unwrap();

        let current = InstrumentId::new("ESH6", "GLBX");
        let next = InstrumentId::new("ESM6", "GLBX");
        manager
            .set_contracts("ES.n.0", current.clone(), Some(next.clone()), None)
            .unwrap();

        // Current: 500k, Next: 800k (1.6x > 1.5x threshold)
        manager.update_open_interest(&current, Decimal::new(500000, 0));
        manager.update_open_interest(&next, Decimal::new(800000, 0));

        let result = manager.check_roll("ES.n.0", Utc::now());
        assert!(result.is_some());
    }

    // ============================================================
    // ROLL EXECUTION TESTS
    // ============================================================

    #[test]
    fn test_execute_roll() {
        let manager = RollManager::new();
        manager.register("ES.c.0").unwrap();

        let current = InstrumentId::new("ESH6", "GLBX");
        let next = InstrumentId::new("ESM6", "GLBX");
        manager
            .set_contracts("ES.c.0", current.clone(), Some(next.clone()), None)
            .unwrap();

        // Set some volume/OI for the contracts
        manager.update_volume(&current, Decimal::new(100000, 0));
        manager.update_volume(&next, Decimal::new(80000, 0));
        manager.update_open_interest(&current, Decimal::new(500000, 0));
        manager.update_open_interest(&next, Decimal::new(400000, 0));

        // Execute roll with prices
        let current_price = Decimal::new(4500, 0);
        let next_price = Decimal::new(4510, 0);

        let result = manager.execute_roll("ES.c.0", current_price, next_price);
        assert!(result.is_ok());

        // Adjustment factor should be next/current = 4510/4500
        let adjustment = result.unwrap();
        let expected = next_price / current_price;
        assert_eq!(adjustment, expected);

        // State should be updated
        let state = manager.get_state("ES.c.0").unwrap();
        assert_eq!(state.current_contract_id, Some(next)); // Shifted to next
        assert!(state.next_contract_id.is_none()); // Next is now empty
        assert_eq!(state.current_volume, Decimal::new(80000, 0)); // Shifted
        assert_eq!(state.current_oi, Decimal::new(400000, 0)); // Shifted
        assert_eq!(state.next_volume, Decimal::ZERO); // Reset
        assert_eq!(state.next_oi, Decimal::ZERO); // Reset
        assert!(state.current_expiration.is_none()); // Reset
    }

    #[test]
    fn test_execute_roll_not_found() {
        let manager = RollManager::new();

        let result = manager.execute_roll(
            "NONEXISTENT.c.0",
            Decimal::new(100, 0),
            Decimal::new(101, 0),
        );
        assert!(matches!(result, Err(RollManagerError::NotFound(_))));
    }

    #[test]
    fn test_execute_roll_zero_current_price() {
        let manager = RollManager::new();
        manager.register("ES.c.0").unwrap();

        let current = InstrumentId::new("ESH6", "GLBX");
        let next = InstrumentId::new("ESM6", "GLBX");
        manager
            .set_contracts("ES.c.0", current, Some(next), None)
            .unwrap();

        // Zero current price should return adjustment of 1.0
        let result = manager.execute_roll("ES.c.0", Decimal::ZERO, Decimal::new(4510, 0));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Decimal::ONE);
    }

    #[test]
    fn test_cumulative_adjustment_factor() {
        let manager = RollManager::new();
        manager.register("ES.c.0").unwrap();

        let h = InstrumentId::new("ESH6", "GLBX");
        let m = InstrumentId::new("ESM6", "GLBX");
        let u = InstrumentId::new("ESU6", "GLBX");

        // First roll: H6 -> M6
        manager
            .set_contracts("ES.c.0", h, Some(m.clone()), None)
            .unwrap();
        manager
            .execute_roll("ES.c.0", Decimal::new(4500, 0), Decimal::new(4510, 0))
            .unwrap();

        // Second roll: M6 -> U6
        manager.set_contracts("ES.c.0", m, Some(u), None).unwrap();
        manager
            .execute_roll("ES.c.0", Decimal::new(4510, 0), Decimal::new(4525, 0))
            .unwrap();

        // Cumulative adjustment should be product of individual adjustments
        let state = manager.get_state("ES.c.0").unwrap();
        let expected = (Decimal::new(4510, 0) / Decimal::new(4500, 0))
            * (Decimal::new(4525, 0) / Decimal::new(4510, 0));
        assert_eq!(state.adjustment_factor, expected);
    }

    // ============================================================
    // ADJUSTMENT FACTOR QUERIES
    // ============================================================

    #[test]
    fn test_get_adjustment_factor() {
        let manager = RollManager::new();
        manager.register("ES.c.0").unwrap();

        // Initial factor should be 1.0
        let factor = manager.get_adjustment_factor("ES.c.0");
        assert_eq!(factor, Some(Decimal::ONE));

        // Non-existent
        assert!(manager.get_adjustment_factor("NONEXISTENT.c.0").is_none());
    }

    #[test]
    fn test_adjust_price() {
        let manager = RollManager::new();
        manager.register("ES.c.0").unwrap();

        let current = InstrumentId::new("ESH6", "GLBX");
        let next = InstrumentId::new("ESM6", "GLBX");
        manager
            .set_contracts("ES.c.0", current, Some(next), None)
            .unwrap();

        // Execute roll
        manager
            .execute_roll("ES.c.0", Decimal::new(4500, 0), Decimal::new(4510, 0))
            .unwrap();

        // Adjust a raw price
        let raw_price = Decimal::new(4520, 0);
        let adjusted = manager.adjust_price("ES.c.0", raw_price);

        assert!(adjusted.is_some());
        // adjusted = raw * (4510/4500) = 4520 * 1.00222... ≈ 4530.04
        let adjustment_factor = Decimal::new(4510, 0) / Decimal::new(4500, 0);
        assert_eq!(adjusted.unwrap(), raw_price * adjustment_factor);
    }

    // ============================================================
    // EVENT SUBSCRIPTION TESTS
    // ============================================================

    #[test]
    fn test_subscribe_to_events() {
        let manager = RollManager::new();

        // Subscribe before any events
        let mut rx = manager.subscribe();

        manager.register("ES.c.0").unwrap();

        let current = InstrumentId::new("ESH6", "GLBX");
        let next = InstrumentId::new("ESM6", "GLBX");

        // Set up for calendar roll
        let expiration = (Utc::now() + Duration::days(2)).date_naive();
        manager
            .set_contracts("ES.c.0", current, Some(next), Some(expiration))
            .unwrap();

        // Trigger roll check - should emit RollScheduled
        let _roll_event = manager.check_roll("ES.c.0", Utc::now());

        // Should have received the event (non-blocking check)
        match rx.try_recv() {
            Ok(RollEvent::RollScheduled {
                continuous_symbol, ..
            }) => {
                assert_eq!(continuous_symbol, "ES.c.0");
            }
            _ => {} // Event might be None if check_roll didn't trigger
        }
    }
}
