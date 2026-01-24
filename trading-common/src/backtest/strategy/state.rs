//! Strategy state management for lifecycle tracking.
//!
//! This module provides types for tracking strategy lifecycle state transitions,
//! inspired by NinjaTrader's state machine model.
//!
//! **Note**: `StrategyState` is now a type alias for `ComponentState` from the
//! unified state management system. This maintains backward compatibility while
//! allowing strategies to participate in the broader component state framework.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::orders::StrategyId;
use crate::state::{ComponentId, ComponentState, ComponentStateEvent, ComponentType};

/// Strategy lifecycle states.
///
/// **Note**: This is now a type alias for `ComponentState`. The unified state
/// system provides the same states plus additional ones for non-data components.
///
/// # State Flow (Data-processing path)
///
/// ```text
/// Undefined → SetDefaults → Configure → DataLoaded → Historical → Transition → Realtime → Terminated → Finalized
///                                                                                              ↓
///                                                                                           Faulted
/// ```
///
/// # Migration
///
/// Existing code using `StrategyState` will continue to work. For new code,
/// consider using `ComponentState` directly from `crate::state`.
pub type StrategyState = ComponentState;

/// Event generated when a strategy's state changes.
///
/// Provides information about the state transition including the old and new
/// states, and an optional reason for the transition.
///
/// **Note**: This type is kept for backward compatibility. New code should
/// consider using `ComponentStateEvent` from `crate::state` directly.
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

/// Convert from ComponentStateEvent to StrategyStateEvent.
///
/// This allows strategies to receive events from the unified state system
/// while maintaining backward compatibility.
impl From<ComponentStateEvent> for StrategyStateEvent {
    fn from(event: ComponentStateEvent) -> Self {
        Self {
            strategy_id: StrategyId::new(&event.component_id.instance_name),
            old_state: event.old_state,
            new_state: event.new_state,
            reason: event.reason,
            ts_event: event.ts_event,
        }
    }
}

/// Convert from StrategyStateEvent to ComponentStateEvent.
///
/// This allows existing strategy events to be published to the unified state registry.
impl From<StrategyStateEvent> for ComponentStateEvent {
    fn from(event: StrategyStateEvent) -> Self {
        let component_id = ComponentId::new(
            ComponentType::Strategy,
            event.strategy_id.as_str().to_string(),
        );

        let mut component_event = Self::new(component_id, event.old_state, event.new_state);
        component_event.reason = event.reason;
        component_event.ts_event = event.ts_event;
        component_event
    }
}

impl StrategyStateEvent {
    /// Create a new state change event
    pub fn new(
        strategy_id: StrategyId,
        old_state: StrategyState,
        new_state: StrategyState,
    ) -> Self {
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

        // Terminated can go to Finalized (unified state system)
        assert!(StrategyState::Terminated.can_transition_to(StrategyState::Finalized));

        // Faulted and Finalized are fully terminal
        assert!(StrategyState::Faulted.valid_transitions().is_empty());
        assert!(StrategyState::Finalized.valid_transitions().is_empty());
    }

    #[test]
    fn test_strategy_state_event_conversion_from_component() {
        let component_id = ComponentId::strategy("my-strategy");
        let component_event = ComponentStateEvent::with_reason(
            component_id,
            StrategyState::Historical,
            StrategyState::Realtime,
            "Warmup complete",
        );

        let strategy_event: StrategyStateEvent = component_event.into();

        assert_eq!(strategy_event.strategy_id.as_str(), "my-strategy");
        assert_eq!(strategy_event.old_state, StrategyState::Historical);
        assert_eq!(strategy_event.new_state, StrategyState::Realtime);
        assert_eq!(strategy_event.reason.as_deref(), Some("Warmup complete"));
    }

    #[test]
    fn test_strategy_state_event_conversion_to_component() {
        let strategy_event = StrategyStateEvent::with_reason(
            StrategyId::new("my-strategy"),
            StrategyState::Configure,
            StrategyState::DataLoaded,
            "Data loaded",
        );

        let component_event: ComponentStateEvent = strategy_event.into();

        assert_eq!(component_event.component_id.instance_name, "my-strategy");
        assert_eq!(
            component_event.component_id.component_type,
            ComponentType::Strategy
        );
        assert_eq!(component_event.old_state, StrategyState::Configure);
        assert_eq!(component_event.new_state, StrategyState::DataLoaded);
        assert_eq!(component_event.reason.as_deref(), Some("Data loaded"));
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
