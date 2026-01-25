//! Error types for the component state management system.

use thiserror::Error;

use super::{ComponentId, ComponentState};
use crate::error::{ErrorCategory, ErrorClassification};

/// Errors that can occur during state management operations.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum StateError {
    /// Component is not registered in the registry
    #[error("Component not registered: {0}")]
    ComponentNotRegistered(ComponentId),

    /// Invalid state transition attempted
    #[error("Invalid state transition for {component_id}: {from} -> {to} ({reason})")]
    InvalidTransition {
        component_id: ComponentId,
        from: ComponentState,
        to: ComponentState,
        reason: String,
    },

    /// Component is already registered
    #[error("Component already registered: {0}")]
    AlreadyRegistered(ComponentId),

    /// Timeout waiting for state change
    #[error("Timeout waiting for {component_id} to reach state {expected_state} (currently {current_state})")]
    Timeout {
        component_id: ComponentId,
        expected_state: ComponentState,
        current_state: ComponentState,
    },

    /// Registry operation failed
    #[error("Registry error: {0}")]
    RegistryError(String),

    /// State transition was rejected by the component
    #[error("State transition to {target} rejected for {component_id}: {reason}")]
    TransitionRejected {
        component_id: ComponentId,
        target: ComponentState,
        reason: String,
    },

    /// Channel communication error
    #[error("Channel error: {0}")]
    ChannelError(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

impl ErrorClassification for StateError {
    fn category(&self) -> ErrorCategory {
        match self {
            StateError::ComponentNotRegistered(_) => ErrorCategory::Permanent,
            StateError::InvalidTransition { .. } => ErrorCategory::Permanent,
            StateError::AlreadyRegistered(_) => ErrorCategory::Permanent,
            StateError::Timeout { .. } => ErrorCategory::Transient,
            StateError::RegistryError(_) => ErrorCategory::Internal,
            StateError::TransitionRejected { .. } => ErrorCategory::Permanent,
            StateError::ChannelError(_) => ErrorCategory::Transient,
            StateError::Internal(_) => ErrorCategory::Internal,
        }
    }

    fn suggested_retry_delay(&self) -> Option<std::time::Duration> {
        match self {
            StateError::Timeout { .. } => Some(std::time::Duration::from_millis(100)),
            StateError::ChannelError(_) => Some(std::time::Duration::from_millis(50)),
            _ => None,
        }
    }
}

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

    #[test]
    fn test_state_error_classification() {
        let id = ComponentId::new(ComponentType::Strategy, "test");

        let err = StateError::Timeout {
            component_id: id.clone(),
            expected_state: ComponentState::Realtime,
            current_state: ComponentState::Configure,
        };
        assert!(err.is_transient());

        let err = StateError::InvalidTransition {
            component_id: id,
            from: ComponentState::Configure,
            to: ComponentState::Realtime,
            reason: "invalid".to_string(),
        };
        assert!(err.is_permanent());
    }
}
