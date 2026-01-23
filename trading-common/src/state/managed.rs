//! StateManaged trait for components with lifecycle management.

use super::events::ComponentStateEvent;
use super::{ComponentId, ComponentState};

/// Trait for components with lifecycle management.
///
/// Implementing this trait allows components to:
/// - Be registered with the `StateRegistry`
/// - Receive notifications of state changes
/// - Query their own state and the state of other components
/// - Participate in hierarchical state propagation
///
/// # Lifecycle Pattern
///
/// Components should handle state changes in `on_state_change`:
///
/// ```ignore
/// fn on_state_change(&mut self, event: &ComponentStateEvent) {
///     match event.new_state {
///         ComponentState::SetDefaults => {
///             // Set default parameters (keep lean - no heavy initialization)
///             self.period = 20;
///         }
///         ComponentState::Configure => {
///             // Configure dependencies, validate parameters
///         }
///         ComponentState::DataLoaded => {
///             // Initialize child indicators/components
///             self.sma = Some(Sma::new(self.period));
///         }
///         ComponentState::Historical => {
///             // Ready for historical data processing
///         }
///         ComponentState::Realtime => {
///             // Ready for live data processing
///         }
///         ComponentState::Terminated => {
///             // Cleanup resources, close connections
///         }
///         _ => {}
///     }
///     self.state = event.new_state;
/// }
/// ```
///
/// # Hierarchical State Propagation
///
/// Parent components can report child IDs for coordinated state management:
///
/// ```ignore
/// fn child_ids(&self) -> Vec<ComponentId> {
///     self.indicators.iter()
///         .map(|ind| ind.component_id().clone())
///         .collect()
/// }
/// ```
pub trait StateManaged: Send + Sync {
    /// Get the component's unique identifier.
    fn component_id(&self) -> &ComponentId;

    /// Get current state.
    fn state(&self) -> ComponentState;

    /// Called when the component's state changes.
    ///
    /// This is the primary hook for components to react to state transitions.
    /// The component should update its internal state field after processing.
    ///
    /// # Arguments
    /// - `event`: The state change event containing old state, new state, and metadata
    fn on_state_change(&mut self, event: &ComponentStateEvent);

    /// Validate if transition to target state is allowed.
    ///
    /// By default, uses the standard state transition rules defined in `ComponentState`.
    /// Override this method to add custom validation logic.
    ///
    /// # Returns
    /// `true` if the transition is valid, `false` otherwise
    fn can_transition_to(&self, target: ComponentState) -> bool {
        self.state().valid_transitions().contains(&target)
    }

    /// Get parent component ID (for hierarchical propagation).
    ///
    /// Returns `None` by default. Override for components that are children
    /// of other components.
    fn parent_id(&self) -> Option<&ComponentId> {
        None
    }

    /// Get child component IDs.
    ///
    /// Returns an empty vector by default. Override for components that
    /// manage child components (e.g., BarsContext managing indicators).
    fn child_ids(&self) -> Vec<ComponentId> {
        Vec::new()
    }

    /// Check if component is ready to process data.
    ///
    /// Returns true if state is Historical, Transition, or Realtime.
    fn is_data_ready(&self) -> bool {
        self.state().can_process_data()
    }

    /// Check if component is in a terminal state.
    ///
    /// Returns true if state is Terminated, Faulted, or Finalized.
    fn is_terminated(&self) -> bool {
        self.state().is_terminal()
    }

    /// Check if component is active (for service-type components).
    ///
    /// Returns true if state is Active.
    fn is_active(&self) -> bool {
        self.state().is_active()
    }
}

/// Extension trait for easier state management operations.
pub trait StateManagedExt: StateManaged {
    /// Check if component is in a specific state.
    fn is_in_state(&self, state: ComponentState) -> bool {
        self.state() == state
    }

    /// Check if component can currently accept orders.
    ///
    /// Returns true if the component is in Realtime or Historical state.
    fn can_accept_orders(&self) -> bool {
        matches!(
            self.state(),
            ComponentState::Historical | ComponentState::Realtime
        )
    }

    /// Check if component is in warmup phase (Historical).
    fn is_warming_up(&self) -> bool {
        self.state() == ComponentState::Historical
    }

    /// Check if component is processing live data.
    fn is_live(&self) -> bool {
        self.state() == ComponentState::Realtime
    }
}

// Blanket implementation for all StateManaged types
impl<T: StateManaged + ?Sized> StateManagedExt for T {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test implementation of StateManaged
    struct TestComponent {
        id: ComponentId,
        state: ComponentState,
        children: Vec<ComponentId>,
        parent: Option<ComponentId>,
        state_history: Vec<ComponentState>,
    }

    impl TestComponent {
        fn new(name: &str) -> Self {
            Self {
                id: ComponentId::strategy(name),
                state: ComponentState::Undefined,
                children: Vec::new(),
                parent: None,
                state_history: Vec::new(),
            }
        }

        fn with_parent(mut self, parent: ComponentId) -> Self {
            self.parent = Some(parent);
            self
        }

        fn with_child(mut self, child: ComponentId) -> Self {
            self.children.push(child);
            self
        }
    }

    impl StateManaged for TestComponent {
        fn component_id(&self) -> &ComponentId {
            &self.id
        }

        fn state(&self) -> ComponentState {
            self.state
        }

        fn on_state_change(&mut self, event: &ComponentStateEvent) {
            self.state_history.push(event.new_state);
            self.state = event.new_state;
        }

        fn parent_id(&self) -> Option<&ComponentId> {
            self.parent.as_ref()
        }

        fn child_ids(&self) -> Vec<ComponentId> {
            self.children.clone()
        }
    }

    #[test]
    fn test_state_managed_basic() {
        let component = TestComponent::new("test");

        assert_eq!(component.component_id().name(), "test");
        assert_eq!(component.state(), ComponentState::Undefined);
        assert!(!component.is_data_ready());
        assert!(!component.is_terminated());
    }

    #[test]
    fn test_state_managed_on_state_change() {
        let mut component = TestComponent::new("test");

        let event = ComponentStateEvent::new(
            component.component_id().clone(),
            ComponentState::Undefined,
            ComponentState::SetDefaults,
        );

        component.on_state_change(&event);
        assert_eq!(component.state(), ComponentState::SetDefaults);
        assert_eq!(component.state_history.len(), 1);
    }

    #[test]
    fn test_state_managed_can_transition_to() {
        let mut component = TestComponent::new("test");

        // From Undefined, can only go to SetDefaults
        assert!(component.can_transition_to(ComponentState::SetDefaults));
        assert!(!component.can_transition_to(ComponentState::Realtime));

        // Move to Configure
        component.state = ComponentState::Configure;

        // From Configure, can go to DataLoaded, Active, or Faulted
        assert!(component.can_transition_to(ComponentState::DataLoaded));
        assert!(component.can_transition_to(ComponentState::Active));
        assert!(component.can_transition_to(ComponentState::Faulted));
        assert!(!component.can_transition_to(ComponentState::Realtime));
    }

    #[test]
    fn test_state_managed_parent_child() {
        let parent_id = ComponentId::strategy("parent");
        let child1_id = ComponentId::indicator("child1");
        let child2_id = ComponentId::indicator("child2");

        let parent = TestComponent::new("parent")
            .with_child(child1_id.clone())
            .with_child(child2_id.clone());

        let child = TestComponent::new("child1").with_parent(parent_id.clone());

        // Verify parent has children
        let children = parent.child_ids();
        assert_eq!(children.len(), 2);
        assert!(children.contains(&child1_id));
        assert!(children.contains(&child2_id));

        // Verify child has parent
        assert_eq!(child.parent_id(), Some(&parent_id));
    }

    #[test]
    fn test_state_managed_ext_is_in_state() {
        let mut component = TestComponent::new("test");

        assert!(component.is_in_state(ComponentState::Undefined));
        assert!(!component.is_in_state(ComponentState::Realtime));

        component.state = ComponentState::Realtime;
        assert!(component.is_in_state(ComponentState::Realtime));
        assert!(!component.is_in_state(ComponentState::Undefined));
    }

    #[test]
    fn test_state_managed_ext_order_states() {
        let mut component = TestComponent::new("test");

        // Undefined - cannot accept orders
        assert!(!component.can_accept_orders());
        assert!(!component.is_warming_up());
        assert!(!component.is_live());

        // Historical - can accept orders, warming up
        component.state = ComponentState::Historical;
        assert!(component.can_accept_orders());
        assert!(component.is_warming_up());
        assert!(!component.is_live());

        // Realtime - can accept orders, live
        component.state = ComponentState::Realtime;
        assert!(component.can_accept_orders());
        assert!(!component.is_warming_up());
        assert!(component.is_live());

        // Terminated - cannot accept orders
        component.state = ComponentState::Terminated;
        assert!(!component.can_accept_orders());
    }

    #[test]
    fn test_state_managed_is_active() {
        let mut component = TestComponent::new("test");

        component.state = ComponentState::Configure;
        assert!(!component.is_active());

        component.state = ComponentState::Active;
        assert!(component.is_active());

        component.state = ComponentState::Realtime;
        assert!(!component.is_active());
    }
}
