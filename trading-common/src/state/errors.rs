//! Error types for the component state management system.

use std::fmt;

use super::{ComponentId, ComponentState};

/// Errors that can occur during state management operations.
#[derive(Debug, Clone)]
pub enum StateError {
    /// Component is not registered in the registry
    ComponentNotRegistered(ComponentId),

    /// Invalid state transition attempted
    InvalidTransition {
        component_id: ComponentId,
        from: ComponentState,
        to: ComponentState,
        reason: String,
    },

    /// Component is already registered
    AlreadyRegistered(ComponentId),

    /// Timeout waiting for state change
    Timeout {
        component_id: ComponentId,
        expected_state: ComponentState,
        current_state: ComponentState,
    },

    /// Registry operation failed
    RegistryError(String),

    /// State transition was rejected by the component
    TransitionRejected {
        component_id: ComponentId,
        target: ComponentState,
        reason: String,
    },

    /// Channel communication error
    ChannelError(String),

    /// Internal error
    Internal(String),
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::ComponentNotRegistered(id) => {
                write!(f, "Component not registered: {}", id)
            }
            StateError::InvalidTransition {
                component_id,
                from,
                to,
                reason,
            } => {
                write!(
                    f,
                    "Invalid state transition for {}: {} -> {} ({})",
                    component_id, from, to, reason
                )
            }
            StateError::AlreadyRegistered(id) => {
                write!(f, "Component already registered: {}", id)
            }
            StateError::Timeout {
                component_id,
                expected_state,
                current_state,
            } => {
                write!(
                    f,
                    "Timeout waiting for {} to reach state {} (currently {})",
                    component_id, expected_state, current_state
                )
            }
            StateError::RegistryError(msg) => {
                write!(f, "Registry error: {}", msg)
            }
            StateError::TransitionRejected {
                component_id,
                target,
                reason,
            } => {
                write!(
                    f,
                    "State transition to {} rejected for {}: {}",
                    target, component_id, reason
                )
            }
            StateError::ChannelError(msg) => {
                write!(f, "Channel error: {}", msg)
            }
            StateError::Internal(msg) => {
                write!(f, "Internal error: {}", msg)
            }
        }
    }
}

impl std::error::Error for StateError {}

/// Result type for state operations
pub type StateResult<T> = Result<T, StateError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ComponentType;

    #[test]
    fn test_state_error_display() {
        let id = ComponentId::new(ComponentType::Strategy, "test-strategy");

        let err = StateError::ComponentNotRegistered(id.clone());
        assert!(err.to_string().contains("not registered"));

        let err = StateError::InvalidTransition {
            component_id: id.clone(),
            from: ComponentState::Configure,
            to: ComponentState::Realtime,
            reason: "must go through DataLoaded first".to_string(),
        };
        assert!(err.to_string().contains("Invalid state transition"));

        let err = StateError::AlreadyRegistered(id.clone());
        assert!(err.to_string().contains("already registered"));

        let err = StateError::Timeout {
            component_id: id,
            expected_state: ComponentState::Realtime,
            current_state: ComponentState::Historical,
        };
        assert!(err.to_string().contains("Timeout"));
    }
}
