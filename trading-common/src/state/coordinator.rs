//! Multi-strategy state coordination system.
//!
//! The `StateCoordinator` manages lifecycle states for multiple component instances,
//! enabling independent warmup and state transitions for each strategy.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                       StateCoordinator                               │
//! │  ┌─────────────────────────────────────────────────────────────┐   │
//! │  │                    SharedStateRegistry                       │   │
//! │  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐        │   │
//! │  │  │ SMA-BTCUSDT  │ │ RSI-BTCUSDT  │ │ SMA-ETHUSDT  │  ...   │   │
//! │  │  │ Historical   │ │ Realtime     │ │ Configure    │        │   │
//! │  │  └──────────────┘ └──────────────┘ └──────────────┘        │   │
//! │  └─────────────────────────────────────────────────────────────┘   │
//! │                                                                     │
//! │  Per-Strategy State:                                                │
//! │  - ComponentId: Strategy:{name}-{symbol}-{instance_id}             │
//! │  - Tracks: first_data_received, warmup_complete, current_state     │
//! │  - Independent warmup/transition per strategy                       │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use trading_common::state::coordinator::{StateCoordinator, StrategyStateTracker};
//! use trading_common::state::{StateRegistry, ComponentState};
//! use std::sync::Arc;
//!
//! let registry = Arc::new(StateRegistry::default());
//! let coordinator = StateCoordinator::new(registry);
//!
//! // Register strategies
//! let id1 = coordinator.register_strategy("SMA", "BTCUSDT").unwrap();
//! let id2 = coordinator.register_strategy("RSI", "BTCUSDT").unwrap();
//!
//! // Transition independently
//! coordinator.transition_to(&id1, ComponentState::SetDefaults, None).unwrap();
//! coordinator.transition_to(&id2, ComponentState::SetDefaults, None).unwrap();
//! ```

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use super::errors::StateResult;
use super::events::ComponentStateEvent;
use super::registry::StateRegistry;
use super::{ComponentId, ComponentState, ComponentType};
use crate::backtest::strategy::state::StrategyStateEvent;
use crate::backtest::strategy::Strategy;
use crate::orders::StrategyId;

/// Tracks per-strategy state during execution.
///
/// Contains both the component lifecycle state and execution-specific flags
/// like `first_data_received` and `warmup_complete`.
#[derive(Debug, Clone)]
pub struct StrategyStateTracker {
    /// Unique component identifier for this strategy instance
    pub component_id: ComponentId,

    /// Strategy identifier (name only, not instance-specific)
    pub strategy_id: StrategyId,

    /// Current component lifecycle state
    pub current_state: ComponentState,

    /// Whether this strategy has received its first bar/tick
    pub first_data_received: bool,

    /// Whether this strategy has completed warmup (is_ready() returned true)
    pub warmup_complete: bool,

    /// Symbol this strategy is trading
    pub symbol: String,

    /// Short instance ID (8 char UUID)
    pub instance_id: String,
}

impl StrategyStateTracker {
    /// Create a new tracker for a strategy instance.
    ///
    /// # Arguments
    /// - `strategy_id`: The strategy's name/identifier
    /// - `symbol`: The trading symbol (e.g., "BTCUSDT")
    /// - `instance_id`: Unique instance identifier (typically a short UUID)
    pub fn new(strategy_id: StrategyId, symbol: &str, instance_id: &str) -> Self {
        let component_id = ComponentId::new(
            ComponentType::Strategy,
            format!("{}-{}-{}", strategy_id.as_str(), symbol, instance_id),
        );

        Self {
            component_id,
            strategy_id,
            current_state: ComponentState::Undefined,
            first_data_received: false,
            warmup_complete: false,
            symbol: symbol.to_string(),
            instance_id: instance_id.to_string(),
        }
    }

    /// Check if strategy is in a data-processing state
    pub fn can_process_data(&self) -> bool {
        self.current_state.can_process_data()
    }

    /// Check if strategy is in realtime mode
    pub fn is_realtime(&self) -> bool {
        self.current_state.is_realtime()
    }

    /// Check if strategy is in a terminal state
    pub fn is_terminal(&self) -> bool {
        self.current_state.is_terminal()
    }

    /// Check if strategy has completed warmup and is ready for live trading
    pub fn is_ready_for_live(&self) -> bool {
        self.warmup_complete && self.current_state.can_process_data()
    }
}

/// Coordinates state management for multiple component instances.
///
/// Wraps a `StateRegistry` and adds:
/// - Per-strategy tracking with execution flags
/// - Strategy registration with unique instance IDs
/// - Transition methods that notify strategies via `on_state_change()`
/// - Query methods for finding strategies in specific states
pub struct StateCoordinator {
    /// Underlying state registry for persistent state storage
    registry: Arc<StateRegistry>,

    /// Per-strategy state trackers keyed by ComponentId
    strategy_trackers: RwLock<HashMap<ComponentId, StrategyStateTracker>>,
}

impl StateCoordinator {
    /// Create a new coordinator with the given registry.
    pub fn new(registry: Arc<StateRegistry>) -> Self {
        Self {
            registry,
            strategy_trackers: RwLock::new(HashMap::new()),
        }
    }

    /// Create a new coordinator with a default registry.
    pub fn with_default_registry() -> Self {
        Self::new(Arc::new(StateRegistry::default()))
    }

    /// Get a reference to the underlying registry.
    pub fn registry(&self) -> &Arc<StateRegistry> {
        &self.registry
    }

    // =========================================================================
    // Registration
    // =========================================================================

    /// Register a new strategy instance.
    ///
    /// Creates a unique ComponentId and registers it with the state registry.
    /// Returns the ComponentId for subsequent operations.
    ///
    /// # Arguments
    /// - `strategy_name`: The strategy's name (e.g., "SMA", "RSI")
    /// - `symbol`: The trading symbol (e.g., "BTCUSDT")
    ///
    /// # Returns
    /// The unique ComponentId for this strategy instance
    pub fn register_strategy(
        &self,
        strategy_name: &str,
        symbol: &str,
    ) -> StateResult<ComponentId> {
        // Generate short instance ID (first 8 chars of UUID)
        let instance_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let strategy_id = StrategyId::new(strategy_name);

        let tracker = StrategyStateTracker::new(strategy_id, symbol, &instance_id);
        let component_id = tracker.component_id.clone();

        // Register in StateRegistry
        self.registry
            .register(component_id.clone(), ComponentState::Undefined)?;

        // Store tracker
        self.strategy_trackers
            .write()
            .insert(component_id.clone(), tracker);

        info!(
            "Registered strategy: {} (symbol: {}, instance: {})",
            component_id, symbol, instance_id
        );

        Ok(component_id)
    }

    /// Register a strategy with a specific instance ID (for deterministic testing).
    pub fn register_strategy_with_id(
        &self,
        strategy_name: &str,
        symbol: &str,
        instance_id: &str,
    ) -> StateResult<ComponentId> {
        let strategy_id = StrategyId::new(strategy_name);
        let tracker = StrategyStateTracker::new(strategy_id, symbol, instance_id);
        let component_id = tracker.component_id.clone();

        // Register in StateRegistry
        self.registry
            .register(component_id.clone(), ComponentState::Undefined)?;

        // Store tracker
        self.strategy_trackers
            .write()
            .insert(component_id.clone(), tracker);

        debug!(
            "Registered strategy: {} (symbol: {}, instance: {})",
            component_id, symbol, instance_id
        );

        Ok(component_id)
    }

    /// Unregister a strategy.
    pub fn unregister_strategy(&self, component_id: &ComponentId) -> StateResult<()> {
        // Remove from registry
        self.registry.unregister(component_id)?;

        // Remove tracker
        self.strategy_trackers.write().remove(component_id);

        debug!("Unregistered strategy: {}", component_id);
        Ok(())
    }

    // =========================================================================
    // State Transitions
    // =========================================================================

    /// Transition a strategy to a new state.
    ///
    /// This method:
    /// 1. Validates the transition via the registry
    /// 2. Updates the tracker's current_state
    /// 3. Returns the event for dispatching to the strategy
    ///
    /// Note: The caller is responsible for calling `strategy.on_state_change()`.
    pub fn transition_to(
        &self,
        component_id: &ComponentId,
        target: ComponentState,
        reason: Option<String>,
    ) -> StateResult<ComponentStateEvent> {
        // Perform transition in registry
        let event = self.registry.transition(component_id, target, reason)?;

        // Update tracker
        if let Some(tracker) = self.strategy_trackers.write().get_mut(component_id) {
            tracker.current_state = target;
        }

        Ok(event)
    }

    /// Transition a strategy and notify it via on_state_change.
    ///
    /// This is a convenience method that handles both the state transition
    /// and the strategy notification.
    pub fn transition_and_notify<S: Strategy + ?Sized>(
        &self,
        component_id: &ComponentId,
        strategy: &mut S,
        target: ComponentState,
        reason: Option<String>,
    ) -> StateResult<()> {
        let event = self.transition_to(component_id, target, reason)?;

        // Convert to strategy event and dispatch
        let strategy_event: StrategyStateEvent = event.into();
        strategy.on_state_change(&strategy_event);

        Ok(())
    }

    /// Force a state transition (bypasses validation).
    ///
    /// Use with caution - primarily for error recovery.
    pub fn force_transition(
        &self,
        component_id: &ComponentId,
        target: ComponentState,
        reason: Option<String>,
    ) -> StateResult<ComponentStateEvent> {
        let event = self.registry.force_transition(component_id, target, reason)?;

        // Update tracker
        if let Some(tracker) = self.strategy_trackers.write().get_mut(component_id) {
            tracker.current_state = target;
        }

        Ok(event)
    }

    /// Transition all registered strategies to a state.
    ///
    /// Useful for shutdown scenarios. Continues even if individual transitions fail.
    pub fn transition_all(
        &self,
        target: ComponentState,
        reason: Option<String>,
    ) -> Vec<(ComponentId, StateResult<ComponentStateEvent>)> {
        let component_ids: Vec<_> = self
            .strategy_trackers
            .read()
            .keys()
            .cloned()
            .collect();

        component_ids
            .into_iter()
            .map(|id| {
                let result = self.transition_to(&id, target, reason.clone());
                (id, result)
            })
            .collect()
    }

    // =========================================================================
    // Tracker Updates
    // =========================================================================

    /// Mark that a strategy has received its first data.
    pub fn mark_data_received(&self, component_id: &ComponentId) {
        if let Some(tracker) = self.strategy_trackers.write().get_mut(component_id) {
            if !tracker.first_data_received {
                tracker.first_data_received = true;
                debug!("Strategy {} received first data", component_id);
            }
        }
    }

    /// Mark that a strategy has completed warmup.
    pub fn mark_warmup_complete(&self, component_id: &ComponentId) {
        if let Some(tracker) = self.strategy_trackers.write().get_mut(component_id) {
            if !tracker.warmup_complete {
                tracker.warmup_complete = true;
                info!("Strategy {} completed warmup", component_id);
            }
        }
    }

    /// Reset warmup flags (for re-running backtests).
    pub fn reset_tracker_flags(&self, component_id: &ComponentId) {
        if let Some(tracker) = self.strategy_trackers.write().get_mut(component_id) {
            tracker.first_data_received = false;
            tracker.warmup_complete = false;
        }
    }

    // =========================================================================
    // Queries
    // =========================================================================

    /// Get the tracker for a component.
    pub fn get_tracker(&self, component_id: &ComponentId) -> Option<StrategyStateTracker> {
        self.strategy_trackers.read().get(component_id).cloned()
    }

    /// Get the current state of a component.
    pub fn get_state(&self, component_id: &ComponentId) -> Option<ComponentState> {
        self.registry.get_state(component_id)
    }

    /// Get all strategies in a specific state.
    pub fn get_strategies_in_state(&self, state: ComponentState) -> Vec<ComponentId> {
        self.strategy_trackers
            .read()
            .iter()
            .filter(|(_, t)| t.current_state == state)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get all strategies for a specific symbol.
    pub fn get_strategies_for_symbol(&self, symbol: &str) -> Vec<ComponentId> {
        self.strategy_trackers
            .read()
            .iter()
            .filter(|(_, t)| t.symbol == symbol)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get all strategies that have completed warmup.
    pub fn get_warmed_up_strategies(&self) -> Vec<ComponentId> {
        self.strategy_trackers
            .read()
            .iter()
            .filter(|(_, t)| t.warmup_complete)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get all strategies that are in realtime mode.
    pub fn get_realtime_strategies(&self) -> Vec<ComponentId> {
        self.strategy_trackers
            .read()
            .iter()
            .filter(|(_, t)| t.is_realtime())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get all registered strategy component IDs.
    pub fn get_all_strategy_ids(&self) -> Vec<ComponentId> {
        self.strategy_trackers.read().keys().cloned().collect()
    }

    /// Get the number of registered strategies.
    pub fn strategy_count(&self) -> usize {
        self.strategy_trackers.read().len()
    }

    /// Check if a strategy is registered.
    pub fn is_registered(&self, component_id: &ComponentId) -> bool {
        self.strategy_trackers.read().contains_key(component_id)
    }

    // =========================================================================
    // Event Subscription
    // =========================================================================

    /// Subscribe to state change events.
    ///
    /// Returns a broadcast receiver that will receive all state change events
    /// for components in the underlying registry.
    pub fn subscribe(&self) -> broadcast::Receiver<ComponentStateEvent> {
        self.registry.subscribe()
    }

    // =========================================================================
    // Convenience Methods for Common Transitions
    // =========================================================================

    /// Perform the standard initialization sequence: Undefined → SetDefaults → Configure
    ///
    /// Returns the Configure event on success.
    pub fn initialize_strategy<S: Strategy + ?Sized>(
        &self,
        component_id: &ComponentId,
        strategy: &mut S,
    ) -> StateResult<()> {
        // Undefined → SetDefaults
        self.transition_and_notify(
            component_id,
            strategy,
            ComponentState::SetDefaults,
            Some("Initialization".to_string()),
        )?;

        // SetDefaults → Configure
        self.transition_and_notify(
            component_id,
            strategy,
            ComponentState::Configure,
            Some("Configuration".to_string()),
        )?;

        Ok(())
    }

    /// Transition strategy into data processing: Configure → DataLoaded → Historical
    ///
    /// Call this when the strategy is about to receive its first bar.
    pub fn enter_data_processing<S: Strategy + ?Sized>(
        &self,
        component_id: &ComponentId,
        strategy: &mut S,
    ) -> StateResult<()> {
        // Configure → DataLoaded
        self.transition_and_notify(
            component_id,
            strategy,
            ComponentState::DataLoaded,
            Some("Data available".to_string()),
        )?;

        self.mark_data_received(component_id);

        // DataLoaded → Historical
        self.transition_and_notify(
            component_id,
            strategy,
            ComponentState::Historical,
            Some("Processing historical data".to_string()),
        )?;

        Ok(())
    }

    /// Transition strategy to realtime: Historical → Transition → Realtime
    ///
    /// Call this when warmup is complete and strategy is ready for live trading.
    pub fn enter_realtime<S: Strategy + ?Sized>(
        &self,
        component_id: &ComponentId,
        strategy: &mut S,
    ) -> StateResult<()> {
        // Historical → Transition
        self.transition_and_notify(
            component_id,
            strategy,
            ComponentState::Transition,
            Some("Warmup complete".to_string()),
        )?;

        self.mark_warmup_complete(component_id);

        // Transition → Realtime
        self.transition_and_notify(
            component_id,
            strategy,
            ComponentState::Realtime,
            Some("Entering realtime".to_string()),
        )?;

        Ok(())
    }

    /// Terminate a strategy: current → Terminated
    ///
    /// Call this during shutdown or when removing a strategy.
    pub fn terminate_strategy<S: Strategy + ?Sized>(
        &self,
        component_id: &ComponentId,
        strategy: &mut S,
        reason: Option<String>,
    ) -> StateResult<()> {
        let current = self.get_state(component_id);

        // Can only terminate from Historical, Transition, Realtime, or Active
        if let Some(state) = current {
            if state.can_transition_to(ComponentState::Terminated) {
                self.transition_and_notify(
                    component_id,
                    strategy,
                    ComponentState::Terminated,
                    reason.or(Some("Shutdown".to_string())),
                )?;
            } else {
                warn!(
                    "Cannot terminate strategy {} from state {}",
                    component_id, state
                );
            }
        }

        Ok(())
    }

    /// Mark a strategy as faulted.
    ///
    /// This is a force transition since Faulted can be reached from most states.
    pub fn fault_strategy<S: Strategy + ?Sized>(
        &self,
        component_id: &ComponentId,
        strategy: &mut S,
        reason: String,
    ) -> StateResult<()> {
        let event = self.force_transition(
            component_id,
            ComponentState::Faulted,
            Some(reason.clone()),
        )?;

        let strategy_event: StrategyStateEvent = event.into();
        strategy.on_state_change(&strategy_event);

        warn!("Strategy {} faulted: {}", component_id, reason);
        Ok(())
    }
}

impl Default for StateCoordinator {
    fn default() -> Self {
        Self::with_default_registry()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_strategy() {
        let coordinator = StateCoordinator::default();

        let id = coordinator
            .register_strategy("SMA", "BTCUSDT")
            .expect("Should register");

        assert!(coordinator.is_registered(&id));
        assert_eq!(coordinator.strategy_count(), 1);

        let tracker = coordinator.get_tracker(&id).expect("Should have tracker");
        assert_eq!(tracker.symbol, "BTCUSDT");
        assert_eq!(tracker.strategy_id.as_str(), "SMA");
        assert_eq!(tracker.current_state, ComponentState::Undefined);
        assert!(!tracker.first_data_received);
        assert!(!tracker.warmup_complete);
    }

    #[test]
    fn test_register_multiple_strategies_unique_ids() {
        let coordinator = StateCoordinator::default();

        let id1 = coordinator
            .register_strategy("SMA", "BTCUSDT")
            .expect("Should register");
        let id2 = coordinator
            .register_strategy("SMA", "BTCUSDT")
            .expect("Should register");
        let id3 = coordinator
            .register_strategy("RSI", "BTCUSDT")
            .expect("Should register");

        // All IDs should be unique even for same strategy+symbol
        assert_ne!(id1, id2);
        assert_ne!(id1, id3);
        assert_ne!(id2, id3);
        assert_eq!(coordinator.strategy_count(), 3);
    }

    #[test]
    fn test_transition_to() {
        let coordinator = StateCoordinator::default();

        let id = coordinator
            .register_strategy_with_id("SMA", "BTCUSDT", "test123")
            .expect("Should register");

        // Undefined → SetDefaults
        let event = coordinator
            .transition_to(&id, ComponentState::SetDefaults, None)
            .expect("Should transition");

        assert_eq!(event.old_state, ComponentState::Undefined);
        assert_eq!(event.new_state, ComponentState::SetDefaults);

        let tracker = coordinator.get_tracker(&id).expect("Should have tracker");
        assert_eq!(tracker.current_state, ComponentState::SetDefaults);
    }

    #[test]
    fn test_invalid_transition() {
        let coordinator = StateCoordinator::default();

        let id = coordinator
            .register_strategy("SMA", "BTCUSDT")
            .expect("Should register");

        // Undefined → Realtime is invalid
        let result = coordinator.transition_to(&id, ComponentState::Realtime, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_mark_data_received() {
        let coordinator = StateCoordinator::default();

        let id = coordinator
            .register_strategy("SMA", "BTCUSDT")
            .expect("Should register");

        assert!(!coordinator
            .get_tracker(&id)
            .unwrap()
            .first_data_received);

        coordinator.mark_data_received(&id);

        assert!(coordinator.get_tracker(&id).unwrap().first_data_received);
    }

    #[test]
    fn test_mark_warmup_complete() {
        let coordinator = StateCoordinator::default();

        let id = coordinator
            .register_strategy("SMA", "BTCUSDT")
            .expect("Should register");

        assert!(!coordinator.get_tracker(&id).unwrap().warmup_complete);

        coordinator.mark_warmup_complete(&id);

        assert!(coordinator.get_tracker(&id).unwrap().warmup_complete);
    }

    #[test]
    fn test_get_strategies_in_state() {
        let coordinator = StateCoordinator::default();

        let id1 = coordinator
            .register_strategy_with_id("SMA", "BTCUSDT", "1")
            .expect("Should register");
        let id2 = coordinator
            .register_strategy_with_id("RSI", "BTCUSDT", "2")
            .expect("Should register");

        // Both start in Undefined
        let undefined = coordinator.get_strategies_in_state(ComponentState::Undefined);
        assert_eq!(undefined.len(), 2);

        // Transition id1 to SetDefaults
        coordinator
            .transition_to(&id1, ComponentState::SetDefaults, None)
            .expect("Should transition");

        let undefined = coordinator.get_strategies_in_state(ComponentState::Undefined);
        assert_eq!(undefined.len(), 1);
        assert!(undefined.contains(&id2));

        let set_defaults = coordinator.get_strategies_in_state(ComponentState::SetDefaults);
        assert_eq!(set_defaults.len(), 1);
        assert!(set_defaults.contains(&id1));
    }

    #[test]
    fn test_get_strategies_for_symbol() {
        let coordinator = StateCoordinator::default();

        let id1 = coordinator
            .register_strategy("SMA", "BTCUSDT")
            .expect("Should register");
        let id2 = coordinator
            .register_strategy("RSI", "ETHUSDT")
            .expect("Should register");
        let id3 = coordinator
            .register_strategy("MACD", "BTCUSDT")
            .expect("Should register");

        let btc_strategies = coordinator.get_strategies_for_symbol("BTCUSDT");
        assert_eq!(btc_strategies.len(), 2);
        assert!(btc_strategies.contains(&id1));
        assert!(btc_strategies.contains(&id3));

        let eth_strategies = coordinator.get_strategies_for_symbol("ETHUSDT");
        assert_eq!(eth_strategies.len(), 1);
        assert!(eth_strategies.contains(&id2));
    }

    #[test]
    fn test_transition_all() {
        let coordinator = StateCoordinator::default();

        let id1 = coordinator
            .register_strategy_with_id("SMA", "BTCUSDT", "1")
            .expect("Should register");
        let id2 = coordinator
            .register_strategy_with_id("RSI", "BTCUSDT", "2")
            .expect("Should register");

        // Move both to SetDefaults
        coordinator
            .transition_to(&id1, ComponentState::SetDefaults, None)
            .expect("Should transition");
        coordinator
            .transition_to(&id2, ComponentState::SetDefaults, None)
            .expect("Should transition");

        // Now try to transition all to Configure
        let results = coordinator.transition_all(ComponentState::Configure, None);

        assert_eq!(results.len(), 2);
        for (_, result) in results {
            assert!(result.is_ok());
        }

        // Both should be in Configure
        let configure = coordinator.get_strategies_in_state(ComponentState::Configure);
        assert_eq!(configure.len(), 2);
    }

    #[test]
    fn test_unregister_strategy() {
        let coordinator = StateCoordinator::default();

        let id = coordinator
            .register_strategy("SMA", "BTCUSDT")
            .expect("Should register");

        assert!(coordinator.is_registered(&id));

        coordinator
            .unregister_strategy(&id)
            .expect("Should unregister");

        assert!(!coordinator.is_registered(&id));
        assert_eq!(coordinator.strategy_count(), 0);
    }

    #[test]
    fn test_reset_tracker_flags() {
        let coordinator = StateCoordinator::default();

        let id = coordinator
            .register_strategy("SMA", "BTCUSDT")
            .expect("Should register");

        coordinator.mark_data_received(&id);
        coordinator.mark_warmup_complete(&id);

        let tracker = coordinator.get_tracker(&id).unwrap();
        assert!(tracker.first_data_received);
        assert!(tracker.warmup_complete);

        coordinator.reset_tracker_flags(&id);

        let tracker = coordinator.get_tracker(&id).unwrap();
        assert!(!tracker.first_data_received);
        assert!(!tracker.warmup_complete);
    }

    #[test]
    fn test_force_transition() {
        let coordinator = StateCoordinator::default();

        let id = coordinator
            .register_strategy("SMA", "BTCUSDT")
            .expect("Should register");

        // Force invalid transition: Undefined → Realtime
        let event = coordinator
            .force_transition(&id, ComponentState::Realtime, Some("Testing".to_string()))
            .expect("Should force transition");

        assert_eq!(event.new_state, ComponentState::Realtime);
        assert_eq!(
            coordinator.get_state(&id),
            Some(ComponentState::Realtime)
        );
    }

    #[test]
    fn test_tracker_helper_methods() {
        let strategy_id = StrategyId::new("SMA");
        let tracker = StrategyStateTracker::new(strategy_id, "BTCUSDT", "test123");

        assert!(!tracker.can_process_data());
        assert!(!tracker.is_realtime());
        assert!(!tracker.is_terminal());
        assert!(!tracker.is_ready_for_live());

        // Simulate warmup complete and Historical state
        let mut tracker = tracker;
        tracker.current_state = ComponentState::Historical;
        tracker.warmup_complete = true;

        assert!(tracker.can_process_data());
        assert!(!tracker.is_realtime());
        assert!(!tracker.is_terminal());
        assert!(tracker.is_ready_for_live());
    }
}
