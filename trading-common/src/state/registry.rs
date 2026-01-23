//! Centralized registry for tracking component states.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use super::errors::{StateError, StateResult};
use super::events::ComponentStateEvent;
use super::{ComponentId, ComponentState, ComponentType};

/// Entry in the state registry for a single component.
#[derive(Debug, Clone)]
pub struct ComponentStateEntry {
    /// Current state
    pub state: ComponentState,

    /// Timestamp when state was last changed
    pub last_changed: DateTime<Utc>,

    /// History of state transitions (limited to last N)
    pub history: Vec<ComponentStateEvent>,

    /// Parent component ID (if any)
    pub parent_id: Option<ComponentId>,

    /// Child component IDs
    pub child_ids: Vec<ComponentId>,
}

impl ComponentStateEntry {
    fn new(initial_state: ComponentState) -> Self {
        Self {
            state: initial_state,
            last_changed: Utc::now(),
            history: Vec::new(),
            parent_id: None,
            child_ids: Vec::new(),
        }
    }

    fn with_parent(mut self, parent_id: ComponentId) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    fn update(&mut self, event: ComponentStateEvent, max_history: usize) {
        self.state = event.new_state;
        self.last_changed = event.ts_event;

        // Maintain history with size limit
        self.history.push(event);
        while self.history.len() > max_history {
            self.history.remove(0);
        }
    }
}

/// Configuration for the StateRegistry.
#[derive(Debug, Clone)]
pub struct RegistryConfig {
    /// Channel capacity for event broadcasting
    pub channel_capacity: usize,

    /// Maximum history entries per component
    pub max_history_per_component: usize,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            channel_capacity: 1024,
            max_history_per_component: 100,
        }
    }
}

/// Centralized registry for tracking component states.
///
/// Enables components to query other components' states and receive
/// notifications of state changes.
///
/// # Features
///
/// - Thread-safe concurrent access using `DashMap`
/// - Event broadcasting for state change notifications
/// - Hierarchical component relationships (parent/child)
/// - State-based and type-based component queries
///
/// # Example
///
/// ```ignore
/// let registry = StateRegistry::new(RegistryConfig::default());
///
/// // Register a component
/// let id = ComponentId::strategy("my-strategy");
/// registry.register(id.clone(), ComponentState::Undefined)?;
///
/// // Subscribe to state changes
/// let mut rx = registry.subscribe();
/// tokio::spawn(async move {
///     while let Ok(event) = rx.recv().await {
///         println!("State change: {} -> {}", event.old_state, event.new_state);
///     }
/// });
///
/// // Transition to a new state
/// registry.transition(&id, ComponentState::SetDefaults, None)?;
///
/// // Query state
/// assert_eq!(registry.get_state(&id), Some(ComponentState::SetDefaults));
/// ```
pub struct StateRegistry {
    /// Component states indexed by ID
    states: DashMap<ComponentId, ComponentStateEntry>,

    /// Parent-child relationships
    hierarchy: DashMap<ComponentId, Vec<ComponentId>>,

    /// Event broadcaster
    event_tx: broadcast::Sender<ComponentStateEvent>,

    /// Configuration
    config: RegistryConfig,
}

impl StateRegistry {
    /// Create a new state registry with default configuration.
    pub fn new(config: RegistryConfig) -> Self {
        let (event_tx, _) = broadcast::channel(config.channel_capacity);
        Self {
            states: DashMap::new(),
            hierarchy: DashMap::new(),
            event_tx,
            config,
        }
    }

    /// Create a new state registry with default settings.
    pub fn default() -> Self {
        Self::new(RegistryConfig::default())
    }

    /// Register a component with an initial state.
    pub fn register(
        &self,
        id: ComponentId,
        initial_state: ComponentState,
    ) -> StateResult<()> {
        if self.states.contains_key(&id) {
            return Err(StateError::AlreadyRegistered(id));
        }

        self.states.insert(id.clone(), ComponentStateEntry::new(initial_state));
        debug!("Registered component: {} with state {:?}", id, initial_state);
        Ok(())
    }

    /// Register a component with a parent relationship.
    pub fn register_with_parent(
        &self,
        id: ComponentId,
        initial_state: ComponentState,
        parent_id: ComponentId,
    ) -> StateResult<()> {
        if self.states.contains_key(&id) {
            return Err(StateError::AlreadyRegistered(id));
        }

        // Add entry with parent reference
        self.states.insert(
            id.clone(),
            ComponentStateEntry::new(initial_state).with_parent(parent_id.clone()),
        );

        // Update parent's children list
        self.hierarchy
            .entry(parent_id.clone())
            .or_default()
            .push(id.clone());

        // Update parent entry's child_ids if it exists
        if let Some(mut parent_entry) = self.states.get_mut(&parent_id) {
            parent_entry.child_ids.push(id.clone());
        }

        debug!("Registered component: {} with parent: {}", id, parent_id);
        Ok(())
    }

    /// Unregister a component.
    pub fn unregister(&self, id: &ComponentId) -> StateResult<()> {
        if self.states.remove(id).is_none() {
            return Err(StateError::ComponentNotRegistered(id.clone()));
        }

        // Remove from parent's children list
        for mut entry in self.hierarchy.iter_mut() {
            entry.value_mut().retain(|child| child != id);
        }

        // Remove this component's children list
        self.hierarchy.remove(id);

        debug!("Unregistered component: {}", id);
        Ok(())
    }

    /// Get current state of a component.
    pub fn get_state(&self, id: &ComponentId) -> Option<ComponentState> {
        self.states.get(id).map(|entry| entry.state)
    }

    /// Get full state entry for a component.
    pub fn get_entry(&self, id: &ComponentId) -> Option<ComponentStateEntry> {
        self.states.get(id).map(|entry| entry.clone())
    }

    /// Request state transition.
    ///
    /// Validates the transition and broadcasts the event if successful.
    pub fn transition(
        &self,
        id: &ComponentId,
        target: ComponentState,
        reason: Option<String>,
    ) -> StateResult<ComponentStateEvent> {
        let mut entry = self
            .states
            .get_mut(id)
            .ok_or_else(|| StateError::ComponentNotRegistered(id.clone()))?;

        let current = entry.state;

        // Validate transition
        if !current.can_transition_to(target) {
            return Err(StateError::InvalidTransition {
                component_id: id.clone(),
                from: current,
                to: target,
                reason: format!(
                    "Invalid transition. Valid targets from {:?}: {:?}",
                    current,
                    current.valid_transitions()
                ),
            });
        }

        // Create event
        let event = if let Some(r) = reason {
            ComponentStateEvent::with_reason(id.clone(), current, target, r)
        } else {
            ComponentStateEvent::new(id.clone(), current, target)
        };

        // Update entry
        entry.update(event.clone(), self.config.max_history_per_component);

        // Broadcast event (ignore send errors - no receivers is ok)
        let _ = self.event_tx.send(event.clone());

        debug!("Component {} transitioned: {:?} -> {:?}", id, current, target);
        Ok(event)
    }

    /// Force a state transition without validation.
    ///
    /// Use with caution - this bypasses the normal state machine rules.
    /// Primarily for error recovery or testing.
    pub fn force_transition(
        &self,
        id: &ComponentId,
        target: ComponentState,
        reason: Option<String>,
    ) -> StateResult<ComponentStateEvent> {
        let mut entry = self
            .states
            .get_mut(id)
            .ok_or_else(|| StateError::ComponentNotRegistered(id.clone()))?;

        let current = entry.state;

        if !current.can_transition_to(target) {
            warn!(
                "Forcing invalid transition for {}: {:?} -> {:?}",
                id, current, target
            );
        }

        let event = if let Some(r) = reason {
            ComponentStateEvent::with_reason(id.clone(), current, target, r)
        } else {
            ComponentStateEvent::new(id.clone(), current, target)
        };

        entry.update(event.clone(), self.config.max_history_per_component);
        let _ = self.event_tx.send(event.clone());

        Ok(event)
    }

    /// Subscribe to state change events.
    pub fn subscribe(&self) -> broadcast::Receiver<ComponentStateEvent> {
        self.event_tx.subscribe()
    }

    /// Wait for a component to reach a specific state.
    ///
    /// Returns immediately if already in the target state.
    pub async fn wait_for_state(
        &self,
        id: &ComponentId,
        target: ComponentState,
        timeout: Duration,
    ) -> StateResult<()> {
        // Check current state first
        if let Some(current) = self.get_state(id) {
            if current == target {
                return Ok(());
            }
        } else {
            return Err(StateError::ComponentNotRegistered(id.clone()));
        }

        // Subscribe and wait
        let mut rx = self.subscribe();
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                let current = self.get_state(id).unwrap_or(ComponentState::Undefined);
                return Err(StateError::Timeout {
                    component_id: id.clone(),
                    expected_state: target,
                    current_state: current,
                });
            }

            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(event)) if event.component_id == *id && event.new_state == target => {
                    return Ok(());
                }
                Ok(Ok(_)) => continue, // Not our event
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue, // Catch up
                Ok(Err(broadcast::error::RecvError::Closed)) => {
                    return Err(StateError::ChannelError("Channel closed".to_string()));
                }
                Err(_) => {
                    let current = self.get_state(id).unwrap_or(ComponentState::Undefined);
                    return Err(StateError::Timeout {
                        component_id: id.clone(),
                        expected_state: target,
                        current_state: current,
                    });
                }
            }
        }
    }

    /// Get all components in a specific state.
    pub fn get_by_state(&self, state: ComponentState) -> Vec<ComponentId> {
        self.states
            .iter()
            .filter(|entry| entry.state == state)
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Get all components of a specific type.
    pub fn get_by_type(&self, component_type: ComponentType) -> Vec<ComponentId> {
        self.states
            .iter()
            .filter(|entry| entry.key().component_type == component_type)
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Get all registered component IDs.
    pub fn get_all_ids(&self) -> Vec<ComponentId> {
        self.states.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Get number of registered components.
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// Check if registry is empty.
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// Get children of a component.
    pub fn get_children(&self, parent_id: &ComponentId) -> Vec<ComponentId> {
        self.hierarchy
            .get(parent_id)
            .map(|children| children.clone())
            .unwrap_or_default()
    }

    /// Get parent of a component.
    pub fn get_parent(&self, id: &ComponentId) -> Option<ComponentId> {
        self.states
            .get(id)
            .and_then(|entry| entry.parent_id.clone())
    }

    /// Clear all registered components.
    pub fn clear(&self) {
        self.states.clear();
        self.hierarchy.clear();
    }
}

// Allow creating StateRegistry from config
impl From<RegistryConfig> for StateRegistry {
    fn from(config: RegistryConfig) -> Self {
        Self::new(config)
    }
}

/// Thread-safe shareable reference to a StateRegistry
pub type SharedStateRegistry = Arc<StateRegistry>;

/// Create a new shared state registry
pub fn shared_registry(config: RegistryConfig) -> SharedStateRegistry {
    Arc::new(StateRegistry::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_id(name: &str) -> ComponentId {
        ComponentId::strategy(name)
    }

    #[test]
    fn test_registry_register() {
        let registry = StateRegistry::default();
        let id = test_id("test");

        registry.register(id.clone(), ComponentState::Undefined).unwrap();
        assert_eq!(registry.get_state(&id), Some(ComponentState::Undefined));
    }

    #[test]
    fn test_registry_duplicate_register() {
        let registry = StateRegistry::default();
        let id = test_id("test");

        registry.register(id.clone(), ComponentState::Undefined).unwrap();

        let result = registry.register(id.clone(), ComponentState::Undefined);
        assert!(matches!(result, Err(StateError::AlreadyRegistered(_))));
    }

    #[test]
    fn test_registry_unregister() {
        let registry = StateRegistry::default();
        let id = test_id("test");

        registry.register(id.clone(), ComponentState::Undefined).unwrap();
        registry.unregister(&id).unwrap();

        assert_eq!(registry.get_state(&id), None);
    }

    #[test]
    fn test_registry_transition() {
        let registry = StateRegistry::default();
        let id = test_id("test");

        registry.register(id.clone(), ComponentState::Undefined).unwrap();

        let event = registry
            .transition(&id, ComponentState::SetDefaults, None)
            .unwrap();

        assert_eq!(event.old_state, ComponentState::Undefined);
        assert_eq!(event.new_state, ComponentState::SetDefaults);
        assert_eq!(registry.get_state(&id), Some(ComponentState::SetDefaults));
    }

    #[test]
    fn test_registry_invalid_transition() {
        let registry = StateRegistry::default();
        let id = test_id("test");

        registry.register(id.clone(), ComponentState::Undefined).unwrap();

        let result = registry.transition(&id, ComponentState::Realtime, None);
        assert!(matches!(result, Err(StateError::InvalidTransition { .. })));
    }

    #[test]
    fn test_registry_transition_with_reason() {
        let registry = StateRegistry::default();
        let id = test_id("test");

        registry.register(id.clone(), ComponentState::Configure).unwrap();

        let event = registry
            .transition(&id, ComponentState::Faulted, Some("Connection lost".to_string()))
            .unwrap();

        assert_eq!(event.reason.as_deref(), Some("Connection lost"));
    }

    #[test]
    fn test_registry_get_by_state() {
        let registry = StateRegistry::default();

        registry
            .register(test_id("a"), ComponentState::Realtime)
            .unwrap();
        registry
            .register(test_id("b"), ComponentState::Realtime)
            .unwrap();
        registry
            .register(test_id("c"), ComponentState::Historical)
            .unwrap();

        let realtime = registry.get_by_state(ComponentState::Realtime);
        assert_eq!(realtime.len(), 2);

        let historical = registry.get_by_state(ComponentState::Historical);
        assert_eq!(historical.len(), 1);
    }

    #[test]
    fn test_registry_get_by_type() {
        let registry = StateRegistry::default();

        registry
            .register(ComponentId::strategy("s1"), ComponentState::Undefined)
            .unwrap();
        registry
            .register(ComponentId::strategy("s2"), ComponentState::Undefined)
            .unwrap();
        registry
            .register(ComponentId::cache("c1"), ComponentState::Undefined)
            .unwrap();

        let strategies = registry.get_by_type(ComponentType::Strategy);
        assert_eq!(strategies.len(), 2);

        let caches = registry.get_by_type(ComponentType::Cache);
        assert_eq!(caches.len(), 1);
    }

    #[test]
    fn test_registry_parent_child() {
        let registry = StateRegistry::default();

        let parent = ComponentId::strategy("parent");
        let child1 = ComponentId::indicator("child1");
        let child2 = ComponentId::indicator("child2");

        registry.register(parent.clone(), ComponentState::Undefined).unwrap();
        registry
            .register_with_parent(child1.clone(), ComponentState::Undefined, parent.clone())
            .unwrap();
        registry
            .register_with_parent(child2.clone(), ComponentState::Undefined, parent.clone())
            .unwrap();

        let children = registry.get_children(&parent);
        assert_eq!(children.len(), 2);
        assert!(children.contains(&child1));
        assert!(children.contains(&child2));

        assert_eq!(registry.get_parent(&child1), Some(parent.clone()));
        assert_eq!(registry.get_parent(&child2), Some(parent));
    }

    #[test]
    fn test_registry_subscribe() {
        let registry = StateRegistry::default();
        let id = test_id("test");

        registry.register(id.clone(), ComponentState::Undefined).unwrap();

        let mut rx = registry.subscribe();

        registry
            .transition(&id, ComponentState::SetDefaults, None)
            .unwrap();

        // Should receive the event
        let event = rx.try_recv().unwrap();
        assert_eq!(event.new_state, ComponentState::SetDefaults);
    }

    #[tokio::test]
    async fn test_registry_wait_for_state() {
        let registry = Arc::new(StateRegistry::default());
        let id = test_id("test");

        registry.register(id.clone(), ComponentState::Undefined).unwrap();

        // Spawn task to transition after delay
        let registry_clone = registry.clone();
        let id_clone = id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            registry_clone
                .transition(&id_clone, ComponentState::SetDefaults, None)
                .unwrap();
        });

        // Wait for state
        let result = registry
            .wait_for_state(&id, ComponentState::SetDefaults, Duration::from_secs(1))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_registry_wait_for_state_timeout() {
        let registry = StateRegistry::default();
        let id = test_id("test");

        registry.register(id.clone(), ComponentState::Undefined).unwrap();

        let result = registry
            .wait_for_state(&id, ComponentState::SetDefaults, Duration::from_millis(50))
            .await;

        assert!(matches!(result, Err(StateError::Timeout { .. })));
    }

    #[tokio::test]
    async fn test_registry_wait_for_state_already_in_state() {
        let registry = StateRegistry::default();
        let id = test_id("test");

        registry.register(id.clone(), ComponentState::SetDefaults).unwrap();

        // Should return immediately
        let result = registry
            .wait_for_state(&id, ComponentState::SetDefaults, Duration::from_millis(10))
            .await;

        assert!(result.is_ok());
    }

    #[test]
    fn test_registry_force_transition() {
        let registry = StateRegistry::default();
        let id = test_id("test");

        registry.register(id.clone(), ComponentState::Undefined).unwrap();

        // Force invalid transition
        let event = registry
            .force_transition(&id, ComponentState::Realtime, Some("Force test".to_string()))
            .unwrap();

        assert_eq!(event.new_state, ComponentState::Realtime);
        assert_eq!(registry.get_state(&id), Some(ComponentState::Realtime));
    }

    #[test]
    fn test_registry_get_entry() {
        let registry = StateRegistry::default();
        let id = test_id("test");

        registry.register(id.clone(), ComponentState::Undefined).unwrap();
        registry.transition(&id, ComponentState::SetDefaults, None).unwrap();
        registry.transition(&id, ComponentState::Configure, None).unwrap();

        let entry = registry.get_entry(&id).unwrap();
        assert_eq!(entry.state, ComponentState::Configure);
        assert_eq!(entry.history.len(), 2);
    }

    #[test]
    fn test_registry_clear() {
        let registry = StateRegistry::default();

        registry.register(test_id("a"), ComponentState::Undefined).unwrap();
        registry.register(test_id("b"), ComponentState::Undefined).unwrap();

        assert_eq!(registry.len(), 2);

        registry.clear();

        assert!(registry.is_empty());
    }
}
