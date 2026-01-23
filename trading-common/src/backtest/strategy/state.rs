//! Strategy state management for lifecycle tracking.
//!
//! This module provides types for tracking strategy lifecycle state transitions,
//! inspired by NinjaTrader's state machine model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::orders::StrategyId;

/// Strategy lifecycle states.
///
/// Tracks the current phase of strategy execution from initialization
/// through to termination. States follow a logical progression with
/// defined transitions.
///
/// # State Flow
///
/// ```text
/// Undefined → SetDefaults → Configure → DataLoaded → Historical → Transition → Realtime → Terminated
///                                                                                      ↓
///                                                                                   Faulted
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StrategyState {
    /// Strategy has not been initialized
    #[default]
    Undefined,

    /// Strategy is setting default parameter values
    SetDefaults,

    /// Strategy is being configured with user parameters
    Configure,

    /// Data feeds have been connected and historical data is available
    DataLoaded,

    /// Processing historical data (backtest warmup period)
    Historical,

    /// Transitioning from historical to realtime processing
    Transition,

    /// Processing live/realtime data
    Realtime,

    /// Strategy has been terminated normally
    Terminated,

    /// Strategy encountered a fatal error
    Faulted,
}

impl StrategyState {
    /// Check if strategy is in a running state (Historical or Realtime)
    pub fn is_running(&self) -> bool {
        matches!(self, StrategyState::Historical | StrategyState::Realtime)
    }

    /// Check if strategy is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(self, StrategyState::Terminated | StrategyState::Faulted)
    }

    /// Check if strategy can accept new data
    pub fn can_process_data(&self) -> bool {
        matches!(
            self,
            StrategyState::Historical | StrategyState::Transition | StrategyState::Realtime
        )
    }

    /// Check if strategy is processing live data
    pub fn is_realtime(&self) -> bool {
        matches!(self, StrategyState::Realtime)
    }

    /// Check if strategy is still initializing
    pub fn is_initializing(&self) -> bool {
        matches!(
            self,
            StrategyState::Undefined
                | StrategyState::SetDefaults
                | StrategyState::Configure
                | StrategyState::DataLoaded
        )
    }

    /// Get valid next states from current state
    pub fn valid_transitions(&self) -> &'static [StrategyState] {
        match self {
            StrategyState::Undefined => &[StrategyState::SetDefaults],
            StrategyState::SetDefaults => &[StrategyState::Configure],
            StrategyState::Configure => &[StrategyState::DataLoaded, StrategyState::Faulted],
            StrategyState::DataLoaded => &[StrategyState::Historical, StrategyState::Faulted],
            StrategyState::Historical => {
                &[StrategyState::Transition, StrategyState::Terminated, StrategyState::Faulted]
            }
            StrategyState::Transition => &[StrategyState::Realtime, StrategyState::Faulted],
            StrategyState::Realtime => &[StrategyState::Terminated, StrategyState::Faulted],
            StrategyState::Terminated => &[],
            StrategyState::Faulted => &[],
        }
    }

    /// Check if transition to target state is valid
    pub fn can_transition_to(&self, target: StrategyState) -> bool {
        self.valid_transitions().contains(&target)
    }
}

impl fmt::Display for StrategyState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StrategyState::Undefined => write!(f, "UNDEFINED"),
            StrategyState::SetDefaults => write!(f, "SET_DEFAULTS"),
            StrategyState::Configure => write!(f, "CONFIGURE"),
            StrategyState::DataLoaded => write!(f, "DATA_LOADED"),
            StrategyState::Historical => write!(f, "HISTORICAL"),
            StrategyState::Transition => write!(f, "TRANSITION"),
            StrategyState::Realtime => write!(f, "REALTIME"),
            StrategyState::Terminated => write!(f, "TERMINATED"),
            StrategyState::Faulted => write!(f, "FAULTED"),
        }
    }
}

/// Event generated when a strategy's state changes.
///
/// Provides information about the state transition including the old and new
/// states, and an optional reason for the transition.
///
/// # Example
///
/// ```ignore
/// fn on_state_change(&mut self, event: &StrategyStateEvent) {
///     if event.new_state == StrategyState::Realtime {
///         println!("Strategy {} is now live!", event.strategy_id);
///     }
///
///     if event.new_state == StrategyState::Faulted {
///         if let Some(reason) = &event.reason {
///             eprintln!("Strategy faulted: {}", reason);
///         }
///     }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyStateEvent {
    /// ID of the strategy that changed state
    pub strategy_id: StrategyId,

    /// Previous state
    pub old_state: StrategyState,

    /// New state after transition
    pub new_state: StrategyState,

    /// Optional reason for the state change
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Timestamp of the state change
    pub ts_event: DateTime<Utc>,
}

impl StrategyStateEvent {
    /// Create a new state change event
    pub fn new(strategy_id: StrategyId, old_state: StrategyState, new_state: StrategyState) -> Self {
        Self {
            strategy_id,
            old_state,
            new_state,
            reason: None,
            ts_event: Utc::now(),
        }
    }

    /// Create a state change event with a reason
    pub fn with_reason(
        strategy_id: StrategyId,
        old_state: StrategyState,
        new_state: StrategyState,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            strategy_id,
            old_state,
            new_state,
            reason: Some(reason.into()),
            ts_event: Utc::now(),
        }
    }

    /// Check if this is a transition to a terminal state
    pub fn is_terminal_transition(&self) -> bool {
        self.new_state.is_terminal()
    }

    /// Check if this is a transition to realtime
    pub fn is_going_live(&self) -> bool {
        self.old_state == StrategyState::Transition && self.new_state == StrategyState::Realtime
    }

    /// Check if this is a fault transition
    pub fn is_fault(&self) -> bool {
        self.new_state == StrategyState::Faulted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_state_default() {
        let state = StrategyState::default();
        assert_eq!(state, StrategyState::Undefined);
    }

    #[test]
    fn test_strategy_state_display() {
        assert_eq!(StrategyState::Undefined.to_string(), "UNDEFINED");
        assert_eq!(StrategyState::SetDefaults.to_string(), "SET_DEFAULTS");
        assert_eq!(StrategyState::Configure.to_string(), "CONFIGURE");
        assert_eq!(StrategyState::DataLoaded.to_string(), "DATA_LOADED");
        assert_eq!(StrategyState::Historical.to_string(), "HISTORICAL");
        assert_eq!(StrategyState::Transition.to_string(), "TRANSITION");
        assert_eq!(StrategyState::Realtime.to_string(), "REALTIME");
        assert_eq!(StrategyState::Terminated.to_string(), "TERMINATED");
        assert_eq!(StrategyState::Faulted.to_string(), "FAULTED");
    }

    #[test]
    fn test_strategy_state_is_running() {
        assert!(!StrategyState::Undefined.is_running());
        assert!(!StrategyState::SetDefaults.is_running());
        assert!(!StrategyState::Configure.is_running());
        assert!(!StrategyState::DataLoaded.is_running());
        assert!(StrategyState::Historical.is_running());
        assert!(!StrategyState::Transition.is_running());
        assert!(StrategyState::Realtime.is_running());
        assert!(!StrategyState::Terminated.is_running());
        assert!(!StrategyState::Faulted.is_running());
    }

    #[test]
    fn test_strategy_state_is_terminal() {
        assert!(!StrategyState::Undefined.is_terminal());
        assert!(!StrategyState::Realtime.is_terminal());
        assert!(StrategyState::Terminated.is_terminal());
        assert!(StrategyState::Faulted.is_terminal());
    }

    #[test]
    fn test_strategy_state_can_process_data() {
        assert!(!StrategyState::Undefined.can_process_data());
        assert!(!StrategyState::Configure.can_process_data());
        assert!(StrategyState::Historical.can_process_data());
        assert!(StrategyState::Transition.can_process_data());
        assert!(StrategyState::Realtime.can_process_data());
        assert!(!StrategyState::Terminated.can_process_data());
    }

    #[test]
    fn test_strategy_state_is_realtime() {
        assert!(!StrategyState::Historical.is_realtime());
        assert!(StrategyState::Realtime.is_realtime());
    }

    #[test]
    fn test_strategy_state_is_initializing() {
        assert!(StrategyState::Undefined.is_initializing());
        assert!(StrategyState::SetDefaults.is_initializing());
        assert!(StrategyState::Configure.is_initializing());
        assert!(StrategyState::DataLoaded.is_initializing());
        assert!(!StrategyState::Historical.is_initializing());
        assert!(!StrategyState::Realtime.is_initializing());
    }

    #[test]
    fn test_strategy_state_valid_transitions() {
        // Undefined can only go to SetDefaults
        assert!(StrategyState::Undefined.can_transition_to(StrategyState::SetDefaults));
        assert!(!StrategyState::Undefined.can_transition_to(StrategyState::Realtime));

        // Historical can go to Transition, Terminated, or Faulted
        assert!(StrategyState::Historical.can_transition_to(StrategyState::Transition));
        assert!(StrategyState::Historical.can_transition_to(StrategyState::Terminated));
        assert!(StrategyState::Historical.can_transition_to(StrategyState::Faulted));
        assert!(!StrategyState::Historical.can_transition_to(StrategyState::Realtime));

        // Terminal states have no valid transitions
        assert!(StrategyState::Terminated.valid_transitions().is_empty());
        assert!(StrategyState::Faulted.valid_transitions().is_empty());
    }

    #[test]
    fn test_strategy_state_event_new() {
        let event = StrategyStateEvent::new(
            StrategyId::new("test-strategy"),
            StrategyState::Historical,
            StrategyState::Realtime,
        );

        assert_eq!(event.strategy_id.as_str(), "test-strategy");
        assert_eq!(event.old_state, StrategyState::Historical);
        assert_eq!(event.new_state, StrategyState::Realtime);
        assert!(event.reason.is_none());
    }

    #[test]
    fn test_strategy_state_event_with_reason() {
        let event = StrategyStateEvent::with_reason(
            StrategyId::new("test-strategy"),
            StrategyState::Realtime,
            StrategyState::Faulted,
            "Connection lost",
        );

        assert_eq!(event.reason.as_deref(), Some("Connection lost"));
        assert!(event.is_fault());
        assert!(event.is_terminal_transition());
    }

    #[test]
    fn test_strategy_state_event_is_going_live() {
        let going_live = StrategyStateEvent::new(
            StrategyId::new("test"),
            StrategyState::Transition,
            StrategyState::Realtime,
        );
        assert!(going_live.is_going_live());

        let not_going_live = StrategyStateEvent::new(
            StrategyId::new("test"),
            StrategyState::Historical,
            StrategyState::Transition,
        );
        assert!(!not_going_live.is_going_live());
    }
}
