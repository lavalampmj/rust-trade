//! Component state change events.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{ComponentId, ComponentState};

/// Event generated when a component's state changes.
///
/// Provides information about the state transition including the old and new
/// states, an optional reason, and metadata for extensibility.
///
/// # Example
///
/// ```ignore
/// fn on_state_change(&mut self, event: &ComponentStateEvent) {
///     match event.new_state {
///         ComponentState::DataLoaded => {
///             // Initialize child indicators
///             self.sma = Some(self.bars.register_indicator("SMA", self.period));
///         }
///         ComponentState::Realtime => {
///             println!("Component {} is now live!", event.component_id);
///         }
///         ComponentState::Faulted => {
///             if let Some(reason) = &event.reason {
///                 eprintln!("Component faulted: {}", reason);
///             }
///         }
///         _ => {}
///     }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentStateEvent {
    /// ID of the component that changed state
    pub component_id: ComponentId,

    /// Previous state
    pub old_state: ComponentState,

    /// New state after transition
    pub new_state: ComponentState,

    /// Optional reason for the state change
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Timestamp of the state change
    pub ts_event: DateTime<Utc>,

    /// Optional metadata for extensibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}

impl ComponentStateEvent {
    /// Create a new state change event
    pub fn new(
        component_id: ComponentId,
        old_state: ComponentState,
        new_state: ComponentState,
    ) -> Self {
        Self {
            component_id,
            old_state,
            new_state,
            reason: None,
            ts_event: Utc::now(),
            metadata: None,
        }
    }

    /// Create a state change event with a reason
    pub fn with_reason(
        component_id: ComponentId,
        old_state: ComponentState,
        new_state: ComponentState,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            component_id,
            old_state,
            new_state,
            reason: Some(reason.into()),
            ts_event: Utc::now(),
            metadata: None,
        }
    }

    /// Add metadata to the event
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata
            .get_or_insert_with(HashMap::new)
            .insert(key.into(), value.into());
        self
    }

    /// Check if this is a transition to a terminal state
    pub fn is_terminal_transition(&self) -> bool {
        self.new_state.is_terminal()
    }

    /// Check if this is a transition to realtime
    pub fn is_going_live(&self) -> bool {
        self.old_state == ComponentState::Transition && self.new_state == ComponentState::Realtime
    }

    /// Check if this is a fault transition
    pub fn is_fault(&self) -> bool {
        self.new_state == ComponentState::Faulted
    }

    /// Check if this is a transition to active state (for non-data components)
    pub fn is_activating(&self) -> bool {
        self.new_state == ComponentState::Active
    }

    /// Check if this is entering data processing states
    pub fn is_entering_data_processing(&self) -> bool {
        matches!(
            self.new_state,
            ComponentState::Historical | ComponentState::Realtime
        ) && !self.old_state.can_process_data()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ComponentType;

    fn test_component_id() -> ComponentId {
        ComponentId::new(ComponentType::Strategy, "test-strategy")
    }

    #[test]
    fn test_component_state_event_new() {
        let event = ComponentStateEvent::new(
            test_component_id(),
            ComponentState::Historical,
            ComponentState::Transition,
        );

        assert_eq!(event.old_state, ComponentState::Historical);
        assert_eq!(event.new_state, ComponentState::Transition);
        assert!(event.reason.is_none());
        assert!(event.metadata.is_none());
    }

    #[test]
    fn test_component_state_event_with_reason() {
        let event = ComponentStateEvent::with_reason(
            test_component_id(),
            ComponentState::Realtime,
            ComponentState::Faulted,
            "Connection lost",
        );

        assert_eq!(event.reason.as_deref(), Some("Connection lost"));
        assert!(event.is_fault());
        assert!(event.is_terminal_transition());
    }

    #[test]
    fn test_component_state_event_with_metadata() {
        let event = ComponentStateEvent::new(
            test_component_id(),
            ComponentState::Configure,
            ComponentState::DataLoaded,
        )
        .with_metadata("symbols", "BTCUSDT,ETHUSDT")
        .with_metadata("timeframe", "1m");

        let metadata = event.metadata.unwrap();
        assert_eq!(metadata.get("symbols").unwrap(), "BTCUSDT,ETHUSDT");
        assert_eq!(metadata.get("timeframe").unwrap(), "1m");
    }

    #[test]
    fn test_component_state_event_is_going_live() {
        let going_live = ComponentStateEvent::new(
            test_component_id(),
            ComponentState::Transition,
            ComponentState::Realtime,
        );
        assert!(going_live.is_going_live());

        let not_going_live = ComponentStateEvent::new(
            test_component_id(),
            ComponentState::Historical,
            ComponentState::Transition,
        );
        assert!(!not_going_live.is_going_live());
    }

    #[test]
    fn test_component_state_event_is_activating() {
        let activating = ComponentStateEvent::new(
            test_component_id(),
            ComponentState::Configure,
            ComponentState::Active,
        );
        assert!(activating.is_activating());

        let not_activating = ComponentStateEvent::new(
            test_component_id(),
            ComponentState::Configure,
            ComponentState::DataLoaded,
        );
        assert!(!not_activating.is_activating());
    }

    #[test]
    fn test_component_state_event_serialization() {
        let event = ComponentStateEvent::with_reason(
            test_component_id(),
            ComponentState::Configure,
            ComponentState::Active,
            "Initialization complete",
        );

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: ComponentStateEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.old_state, ComponentState::Configure);
        assert_eq!(deserialized.new_state, ComponentState::Active);
        assert_eq!(
            deserialized.reason.as_deref(),
            Some("Initialization complete")
        );
    }
}
