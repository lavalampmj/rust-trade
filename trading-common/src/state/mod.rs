//! Unified component state management system.
//!
//! This module provides a formal component lifecycle pattern inspired by NinjaTrader's
//! state machine model. It applies to ALL framework components including strategies,
//! indicators, data sources, caches, and services.
//!
//! # Key Design Principles
//!
//! 1. **Unified State Enum**: Single `ComponentState` applies to all component types
//! 2. **State Query Mechanism**: Components can check other components' states via registry
//! 3. **Deterministic Transitions**: Valid state transitions are enforced
//! 4. **OnStateChange Callback**: Components receive notifications of their own state changes
//! 5. **SetDefaults Kept Lean**: Minimal work in SetDefaults (UI instantiation)
//! 6. **DataLoaded for Child Creation**: Instantiate child indicators/components in DataLoaded
//! 7. **Terminated for Cleanup**: Resource release happens in Terminated
//!
//! # State Flow
//!
//! ```text
//! Data-processing components (strategies, indicators):
//!   Undefined → SetDefaults → Configure → DataLoaded → Historical → Transition → Realtime → Terminated → Finalized
//!
//! Non-data components (services, caches, adapters):
//!   Undefined → SetDefaults → Configure → Active → Terminated → Finalized
//!
//! Error recovery:
//!   Any state → Faulted (terminal)
//! ```
//!
//! # Example
//!
//! ```ignore
//! use trading_common::state::{ComponentState, ComponentId, ComponentType, StateManaged};
//!
//! struct MyStrategy {
//!     state: ComponentState,
//!     component_id: ComponentId,
//!     // ... other fields
//! }
//!
//! impl StateManaged for MyStrategy {
//!     fn component_id(&self) -> &ComponentId {
//!         &self.component_id
//!     }
//!
//!     fn state(&self) -> ComponentState {
//!         self.state
//!     }
//!
//!     fn on_state_change(&mut self, event: &ComponentStateEvent) {
//!         match event.new_state {
//!             ComponentState::SetDefaults => {
//!                 // Set default parameters (keep lean)
//!                 self.period = 20;
//!             }
//!             ComponentState::DataLoaded => {
//!                 // Initialize child indicators
//!                 self.sma = Some(Sma::new(self.period));
//!             }
//!             ComponentState::Terminated => {
//!                 // Cleanup resources
//!             }
//!             _ => {}
//!         }
//!         self.state = event.new_state;
//!     }
//! }
//! ```

pub mod errors;
pub mod events;
pub mod managed;
pub mod registry;

pub use errors::{StateError, StateResult};
pub use events::ComponentStateEvent;
pub use managed::StateManaged;
pub use registry::StateRegistry;

use serde::{Deserialize, Serialize};
use std::fmt;

/// Unified component lifecycle states (NinjaTrader pattern).
///
/// Applies to ALL component types: strategies, indicators, data sources, caches, services.
///
/// # State Categories
///
/// - **Initialization**: Undefined, SetDefaults, Configure
/// - **Data-ready**: DataLoaded, Active
/// - **Processing**: Historical, Transition, Realtime
/// - **Terminal**: Terminated, Faulted, Finalized
///
/// # State Transitions
///
/// ```text
/// Undefined → SetDefaults
/// SetDefaults → Configure
/// Configure → Active | DataLoaded | Faulted
/// Active → Terminated | Faulted
/// DataLoaded → Historical | Faulted
/// Historical → Transition | Terminated | Faulted
/// Transition → Realtime | Faulted
/// Realtime → Terminated | Faulted
/// Terminated → Finalized
/// Finalized → (terminal)
/// Faulted → (terminal)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ComponentState {
    /// Component has not been initialized
    #[default]
    Undefined,

    /// Setting default property values (keep lean - UI instantiation)
    SetDefaults,

    /// Adding data series, configuring dependencies
    Configure,

    /// For non-data components (services, adapters, caches) - equivalent to "running"
    Active,

    /// All data series loaded - instantiate child indicators here
    DataLoaded,

    /// Processing historical data (backtest warmup period)
    Historical,

    /// Switching from historical to realtime processing
    Transition,

    /// Processing live/realtime data
    Realtime,

    /// Normal shutdown initiated - cleanup resources here
    Terminated,

    /// Fatal error occurred
    Faulted,

    /// Internal cleanup complete (terminal state)
    Finalized,
}

impl ComponentState {
    /// Check if component is in a running state (Historical, Realtime, or Active)
    pub fn is_running(&self) -> bool {
        matches!(
            self,
            ComponentState::Historical | ComponentState::Realtime | ComponentState::Active
        )
    }

    /// Check if component is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ComponentState::Terminated | ComponentState::Faulted | ComponentState::Finalized
        )
    }

    /// Check if component can accept new data
    pub fn can_process_data(&self) -> bool {
        matches!(
            self,
            ComponentState::Historical | ComponentState::Transition | ComponentState::Realtime
        )
    }

    /// Check if component is processing live data
    pub fn is_realtime(&self) -> bool {
        matches!(self, ComponentState::Realtime)
    }

    /// Check if component is still initializing
    pub fn is_initializing(&self) -> bool {
        matches!(
            self,
            ComponentState::Undefined
                | ComponentState::SetDefaults
                | ComponentState::Configure
                | ComponentState::DataLoaded
        )
    }

    /// Check if component is active (for non-data components)
    pub fn is_active(&self) -> bool {
        matches!(self, ComponentState::Active)
    }

    /// Get valid next states from current state
    pub fn valid_transitions(&self) -> &'static [ComponentState] {
        match self {
            ComponentState::Undefined => &[ComponentState::SetDefaults],
            ComponentState::SetDefaults => &[ComponentState::Configure],
            ComponentState::Configure => &[
                ComponentState::Active,
                ComponentState::DataLoaded,
                ComponentState::Faulted,
            ],
            ComponentState::Active => &[ComponentState::Terminated, ComponentState::Faulted],
            ComponentState::DataLoaded => &[ComponentState::Historical, ComponentState::Faulted],
            ComponentState::Historical => &[
                ComponentState::Transition,
                ComponentState::Terminated,
                ComponentState::Faulted,
            ],
            ComponentState::Transition => &[ComponentState::Realtime, ComponentState::Faulted],
            ComponentState::Realtime => &[ComponentState::Terminated, ComponentState::Faulted],
            ComponentState::Terminated => &[ComponentState::Finalized],
            ComponentState::Faulted => &[],
            ComponentState::Finalized => &[],
        }
    }

    /// Check if transition to target state is valid
    pub fn can_transition_to(&self, target: ComponentState) -> bool {
        self.valid_transitions().contains(&target)
    }

    /// Convert to integer for FFI/Python bridge
    pub fn as_i32(&self) -> i32 {
        match self {
            ComponentState::Undefined => 0,
            ComponentState::SetDefaults => 1,
            ComponentState::Configure => 2,
            ComponentState::Active => 3,
            ComponentState::DataLoaded => 4,
            ComponentState::Historical => 5,
            ComponentState::Transition => 6,
            ComponentState::Realtime => 7,
            ComponentState::Terminated => 8,
            ComponentState::Faulted => 9,
            ComponentState::Finalized => 10,
        }
    }

    /// Create from integer (for FFI/Python bridge)
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(ComponentState::Undefined),
            1 => Some(ComponentState::SetDefaults),
            2 => Some(ComponentState::Configure),
            3 => Some(ComponentState::Active),
            4 => Some(ComponentState::DataLoaded),
            5 => Some(ComponentState::Historical),
            6 => Some(ComponentState::Transition),
            7 => Some(ComponentState::Realtime),
            8 => Some(ComponentState::Terminated),
            9 => Some(ComponentState::Faulted),
            10 => Some(ComponentState::Finalized),
            _ => None,
        }
    }
}

impl fmt::Display for ComponentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComponentState::Undefined => write!(f, "UNDEFINED"),
            ComponentState::SetDefaults => write!(f, "SET_DEFAULTS"),
            ComponentState::Configure => write!(f, "CONFIGURE"),
            ComponentState::Active => write!(f, "ACTIVE"),
            ComponentState::DataLoaded => write!(f, "DATA_LOADED"),
            ComponentState::Historical => write!(f, "HISTORICAL"),
            ComponentState::Transition => write!(f, "TRANSITION"),
            ComponentState::Realtime => write!(f, "REALTIME"),
            ComponentState::Terminated => write!(f, "TERMINATED"),
            ComponentState::Faulted => write!(f, "FAULTED"),
            ComponentState::Finalized => write!(f, "FINALIZED"),
        }
    }
}

/// Component type discriminator.
///
/// Identifies the category of component for type-based filtering and routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ComponentType {
    /// Trading strategy
    Strategy,

    /// Indicator or Series<T>
    Indicator,

    /// BarsContext for OHLCV data
    BarsContext,

    /// Cache (InMemoryTickCache, RedisTickCache, TieredCache)
    Cache,

    /// Data source (IpcExchange, NatsExchange, WebSocket)
    DataSource,

    /// Repository (TickDataRepository)
    Repository,

    /// Execution engine
    Execution,

    /// Session manager or roll manager
    Session,

    /// Symbol registry or resolver
    Symbol,

    /// Service (MarketDataService, AlertEvaluator)
    Service,

    /// Risk engine or fee model
    Risk,

    /// Order manager
    OrderManager,

    /// Custom extension point
    Custom(u16),
}

impl fmt::Display for ComponentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComponentType::Strategy => write!(f, "Strategy"),
            ComponentType::Indicator => write!(f, "Indicator"),
            ComponentType::BarsContext => write!(f, "BarsContext"),
            ComponentType::Cache => write!(f, "Cache"),
            ComponentType::DataSource => write!(f, "DataSource"),
            ComponentType::Repository => write!(f, "Repository"),
            ComponentType::Execution => write!(f, "Execution"),
            ComponentType::Session => write!(f, "Session"),
            ComponentType::Symbol => write!(f, "Symbol"),
            ComponentType::Service => write!(f, "Service"),
            ComponentType::Risk => write!(f, "Risk"),
            ComponentType::OrderManager => write!(f, "OrderManager"),
            ComponentType::Custom(id) => write!(f, "Custom({})", id),
        }
    }
}

/// Unique identifier for a component instance.
///
/// Combines component type with an instance name for unique identification
/// within the state registry.
///
/// # Example
///
/// ```ignore
/// let strategy_id = ComponentId::new(ComponentType::Strategy, "my-sma-strategy");
/// let cache_id = ComponentId::cache("btcusdt-tick-cache");
/// let indicator_id = ComponentId::indicator("BTCUSDT:SMA20");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ComponentId {
    /// The type of component
    pub component_type: ComponentType,

    /// Unique instance name within the type
    pub instance_name: String,
}

impl ComponentId {
    /// Create a new component ID
    pub fn new(component_type: ComponentType, instance_name: impl Into<String>) -> Self {
        Self {
            component_type,
            instance_name: instance_name.into(),
        }
    }

    /// Create a strategy component ID
    pub fn strategy(name: impl Into<String>) -> Self {
        Self::new(ComponentType::Strategy, name)
    }

    /// Create an indicator component ID
    pub fn indicator(name: impl Into<String>) -> Self {
        Self::new(ComponentType::Indicator, name)
    }

    /// Create a cache component ID
    pub fn cache(name: impl Into<String>) -> Self {
        Self::new(ComponentType::Cache, name)
    }

    /// Create a data source component ID
    pub fn data_source(name: impl Into<String>) -> Self {
        Self::new(ComponentType::DataSource, name)
    }

    /// Create a service component ID
    pub fn service(name: impl Into<String>) -> Self {
        Self::new(ComponentType::Service, name)
    }

    /// Create a bars context component ID
    pub fn bars_context(symbol: impl Into<String>) -> Self {
        Self::new(ComponentType::BarsContext, symbol)
    }

    /// Get the instance name
    pub fn name(&self) -> &str {
        &self.instance_name
    }
}

impl fmt::Display for ComponentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.component_type, self.instance_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_state_default() {
        let state = ComponentState::default();
        assert_eq!(state, ComponentState::Undefined);
    }

    #[test]
    fn test_component_state_display() {
        assert_eq!(ComponentState::Undefined.to_string(), "UNDEFINED");
        assert_eq!(ComponentState::SetDefaults.to_string(), "SET_DEFAULTS");
        assert_eq!(ComponentState::Configure.to_string(), "CONFIGURE");
        assert_eq!(ComponentState::Active.to_string(), "ACTIVE");
        assert_eq!(ComponentState::DataLoaded.to_string(), "DATA_LOADED");
        assert_eq!(ComponentState::Historical.to_string(), "HISTORICAL");
        assert_eq!(ComponentState::Transition.to_string(), "TRANSITION");
        assert_eq!(ComponentState::Realtime.to_string(), "REALTIME");
        assert_eq!(ComponentState::Terminated.to_string(), "TERMINATED");
        assert_eq!(ComponentState::Faulted.to_string(), "FAULTED");
        assert_eq!(ComponentState::Finalized.to_string(), "FINALIZED");
    }

    #[test]
    fn test_component_state_is_running() {
        assert!(!ComponentState::Undefined.is_running());
        assert!(!ComponentState::SetDefaults.is_running());
        assert!(!ComponentState::Configure.is_running());
        assert!(ComponentState::Active.is_running());
        assert!(!ComponentState::DataLoaded.is_running());
        assert!(ComponentState::Historical.is_running());
        assert!(!ComponentState::Transition.is_running());
        assert!(ComponentState::Realtime.is_running());
        assert!(!ComponentState::Terminated.is_running());
        assert!(!ComponentState::Faulted.is_running());
        assert!(!ComponentState::Finalized.is_running());
    }

    #[test]
    fn test_component_state_is_terminal() {
        assert!(!ComponentState::Undefined.is_terminal());
        assert!(!ComponentState::Realtime.is_terminal());
        assert!(!ComponentState::Active.is_terminal());
        assert!(ComponentState::Terminated.is_terminal());
        assert!(ComponentState::Faulted.is_terminal());
        assert!(ComponentState::Finalized.is_terminal());
    }

    #[test]
    fn test_component_state_can_process_data() {
        assert!(!ComponentState::Undefined.can_process_data());
        assert!(!ComponentState::Configure.can_process_data());
        assert!(!ComponentState::Active.can_process_data());
        assert!(ComponentState::Historical.can_process_data());
        assert!(ComponentState::Transition.can_process_data());
        assert!(ComponentState::Realtime.can_process_data());
        assert!(!ComponentState::Terminated.can_process_data());
    }

    #[test]
    fn test_component_state_is_realtime() {
        assert!(!ComponentState::Historical.is_realtime());
        assert!(ComponentState::Realtime.is_realtime());
    }

    #[test]
    fn test_component_state_is_initializing() {
        assert!(ComponentState::Undefined.is_initializing());
        assert!(ComponentState::SetDefaults.is_initializing());
        assert!(ComponentState::Configure.is_initializing());
        assert!(ComponentState::DataLoaded.is_initializing());
        assert!(!ComponentState::Historical.is_initializing());
        assert!(!ComponentState::Realtime.is_initializing());
        assert!(!ComponentState::Active.is_initializing());
    }

    #[test]
    fn test_component_state_valid_transitions_data_processing_path() {
        // Full data-processing path
        assert!(ComponentState::Undefined.can_transition_to(ComponentState::SetDefaults));
        assert!(ComponentState::SetDefaults.can_transition_to(ComponentState::Configure));
        assert!(ComponentState::Configure.can_transition_to(ComponentState::DataLoaded));
        assert!(ComponentState::DataLoaded.can_transition_to(ComponentState::Historical));
        assert!(ComponentState::Historical.can_transition_to(ComponentState::Transition));
        assert!(ComponentState::Transition.can_transition_to(ComponentState::Realtime));
        assert!(ComponentState::Realtime.can_transition_to(ComponentState::Terminated));
        assert!(ComponentState::Terminated.can_transition_to(ComponentState::Finalized));
    }

    #[test]
    fn test_component_state_valid_transitions_service_path() {
        // Service path (Configure → Active)
        assert!(ComponentState::Configure.can_transition_to(ComponentState::Active));
        assert!(ComponentState::Active.can_transition_to(ComponentState::Terminated));
    }

    #[test]
    fn test_component_state_valid_transitions_faulted() {
        // Faulted can be reached from most non-terminal states
        assert!(ComponentState::Configure.can_transition_to(ComponentState::Faulted));
        assert!(ComponentState::DataLoaded.can_transition_to(ComponentState::Faulted));
        assert!(ComponentState::Historical.can_transition_to(ComponentState::Faulted));
        assert!(ComponentState::Transition.can_transition_to(ComponentState::Faulted));
        assert!(ComponentState::Realtime.can_transition_to(ComponentState::Faulted));
        assert!(ComponentState::Active.can_transition_to(ComponentState::Faulted));

        // Faulted is terminal
        assert!(ComponentState::Faulted.valid_transitions().is_empty());
    }

    #[test]
    fn test_component_state_invalid_transitions() {
        // Cannot skip states
        assert!(!ComponentState::Undefined.can_transition_to(ComponentState::Realtime));
        assert!(!ComponentState::Configure.can_transition_to(ComponentState::Realtime));
        assert!(!ComponentState::Historical.can_transition_to(ComponentState::Realtime));

        // Cannot go backwards
        assert!(!ComponentState::Realtime.can_transition_to(ComponentState::Historical));
        assert!(!ComponentState::Active.can_transition_to(ComponentState::Configure));
    }

    #[test]
    fn test_component_state_as_i32() {
        assert_eq!(ComponentState::Undefined.as_i32(), 0);
        assert_eq!(ComponentState::SetDefaults.as_i32(), 1);
        assert_eq!(ComponentState::Configure.as_i32(), 2);
        assert_eq!(ComponentState::Active.as_i32(), 3);
        assert_eq!(ComponentState::Finalized.as_i32(), 10);
    }

    #[test]
    fn test_component_state_from_i32() {
        assert_eq!(ComponentState::from_i32(0), Some(ComponentState::Undefined));
        assert_eq!(
            ComponentState::from_i32(7),
            Some(ComponentState::Realtime)
        );
        assert_eq!(ComponentState::from_i32(10), Some(ComponentState::Finalized));
        assert_eq!(ComponentState::from_i32(11), None);
        assert_eq!(ComponentState::from_i32(-1), None);
    }

    #[test]
    fn test_component_type_display() {
        assert_eq!(ComponentType::Strategy.to_string(), "Strategy");
        assert_eq!(ComponentType::Cache.to_string(), "Cache");
        assert_eq!(ComponentType::Custom(42).to_string(), "Custom(42)");
    }

    #[test]
    fn test_component_id_new() {
        let id = ComponentId::new(ComponentType::Strategy, "my-strategy");
        assert_eq!(id.component_type, ComponentType::Strategy);
        assert_eq!(id.instance_name, "my-strategy");
        assert_eq!(id.name(), "my-strategy");
    }

    #[test]
    fn test_component_id_factory_methods() {
        let strategy = ComponentId::strategy("sma-20");
        assert_eq!(strategy.component_type, ComponentType::Strategy);

        let indicator = ComponentId::indicator("RSI:BTCUSDT");
        assert_eq!(indicator.component_type, ComponentType::Indicator);

        let cache = ComponentId::cache("tick-cache");
        assert_eq!(cache.component_type, ComponentType::Cache);

        let data_source = ComponentId::data_source("binance-ws");
        assert_eq!(data_source.component_type, ComponentType::DataSource);

        let service = ComponentId::service("market-data");
        assert_eq!(service.component_type, ComponentType::Service);
    }

    #[test]
    fn test_component_id_display() {
        let id = ComponentId::new(ComponentType::Strategy, "my-strategy");
        assert_eq!(id.to_string(), "Strategy:my-strategy");

        let cache_id = ComponentId::cache("tick-cache");
        assert_eq!(cache_id.to_string(), "Cache:tick-cache");
    }

    #[test]
    fn test_component_id_equality() {
        let id1 = ComponentId::strategy("test");
        let id2 = ComponentId::strategy("test");
        let id3 = ComponentId::strategy("other");
        let id4 = ComponentId::indicator("test");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert_ne!(id1, id4);
    }

    #[test]
    fn test_component_id_serialization() {
        let id = ComponentId::strategy("my-strategy");
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: ComponentId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    #[test]
    fn test_component_state_serialization() {
        let state = ComponentState::Realtime;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"REALTIME\"");

        let deserialized: ComponentState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, ComponentState::Realtime);
    }
}
