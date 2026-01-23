# Implementation Plan: Unified Component State Management System

## Overview

Redesign the current `StrategyState`/`StrategyStateEvent` to implement NinjaTrader's formal component state lifecycle pattern. This applies to ALL framework components (strategies, indicators, transforms, data sources, caches, services) - not just strategies.

### Key Design Principles (from NinjaTrader)

1. **Unified State Enum**: Single `ComponentState` applies to all component types
2. **State Query Mechanism**: Components can check other components' states
3. **Deterministic Transitions**: Valid state transitions are enforced
4. **OnStateChange Callback**: Components receive notifications of their own state changes
5. **SetDefaults Kept Lean**: Minimal work in SetDefaults (UI instantiation)
6. **DataLoaded for Child Creation**: Instantiate child indicators/components in DataLoaded
7. **Terminated for Cleanup**: Resource release happens in Terminated

### NinjaTrader State Flow

```
SetDefaults → Configure → [Active|DataLoaded] → Historical → Transition → Realtime → Terminated → Finalized
                              ↓                                                           ↓
                           (non-data)                                                  Faulted
```

---

## Phase 1: Core Types (trading-common/src/state/)

### 1.1 Create State Module Structure

**New files:**
```
trading-common/src/state/
├── mod.rs           # ComponentState, ComponentId, ComponentType, re-exports
├── managed.rs       # StateManaged trait
├── registry.rs      # StateRegistry for centralized state tracking
├── events.rs        # ComponentStateEvent
└── errors.rs        # StateError types
```

### 1.2 ComponentState Enum

**File: `trading-common/src/state/mod.rs`**

```rust
/// Unified component lifecycle states (NinjaTrader pattern).
/// Applies to ALL component types: strategies, indicators, data sources, caches, services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ComponentState {
    #[default]
    Undefined,      // Not initialized
    SetDefaults,    // Setting default property values (keep lean)
    Configure,      // Adding data series, configuring dependencies
    Active,         // For non-data components (services, adapters) - equivalent to "running"
    DataLoaded,     // All data series loaded, instantiate child indicators
    Historical,     // Processing historical data (backtest warmup)
    Transition,     // Switching from historical to realtime
    Realtime,       // Processing live data
    Terminated,     // Normal shutdown initiated
    Faulted,        // Fatal error
    Finalized,      // Internal cleanup complete
}

impl ComponentState {
    pub fn is_running(&self) -> bool { ... }
    pub fn is_terminal(&self) -> bool { ... }
    pub fn can_process_data(&self) -> bool { ... }
    pub fn valid_transitions(&self) -> &'static [ComponentState] { ... }
    pub fn can_transition_to(&self, target: ComponentState) -> bool { ... }
}
```

### 1.3 ComponentId and ComponentType

```rust
/// Component type discriminator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComponentType {
    Strategy,
    Indicator,      // Series<T>, custom indicators
    BarsContext,
    Cache,          // InMemoryTickCache, RedisTickCache, TieredCache
    DataSource,     // IpcExchange, NatsExchange
    Repository,     // TickDataRepository
    Execution,      // ExecutionEngine
    Session,        // SessionManager, RollManager
    Symbol,         // SymbolRegistry, SymbolResolver
    Service,        // MarketDataService, AlertEvaluator
    Custom(u16),    // Extension point
}

/// Unique identifier for a component instance
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ComponentId {
    pub component_type: ComponentType,
    pub instance_name: String,
}
```

### 1.4 ComponentStateEvent

**File: `trading-common/src/state/events.rs`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentStateEvent {
    pub component_id: ComponentId,
    pub old_state: ComponentState,
    pub new_state: ComponentState,
    pub reason: Option<String>,
    pub ts_event: DateTime<Utc>,
    pub metadata: Option<HashMap<String, String>>,
}
```

---

## Phase 2: StateManaged Trait

**File: `trading-common/src/state/managed.rs`**

```rust
/// Trait for components with lifecycle management.
pub trait StateManaged: Send + Sync {
    /// Get the component's unique identifier
    fn component_id(&self) -> &ComponentId;

    /// Get current state
    fn state(&self) -> ComponentState;

    /// Called when the component's state changes
    fn on_state_change(&mut self, event: &ComponentStateEvent);

    /// Validate if transition to target state is allowed
    fn can_transition_to(&self, target: ComponentState) -> bool {
        self.state().valid_transitions().contains(&target)
    }

    /// Get parent component ID (for hierarchical propagation)
    fn parent_id(&self) -> Option<&ComponentId> { None }

    /// Get child component IDs
    fn child_ids(&self) -> Vec<ComponentId> { Vec::new() }
}
```

---

## Phase 3: StateRegistry

**File: `trading-common/src/state/registry.rs`**

```rust
/// Centralized registry for tracking component states.
/// Enables components to query other components' states.
pub struct StateRegistry {
    states: DashMap<ComponentId, ComponentStateEntry>,
    event_tx: broadcast::Sender<ComponentStateEvent>,
    hierarchy: DashMap<ComponentId, Vec<ComponentId>>,
}

impl StateRegistry {
    pub fn new(channel_capacity: usize) -> Self;

    /// Register a component
    pub fn register(&self, id: ComponentId, initial_state: ComponentState) -> Result<(), StateError>;

    /// Get current state of any component
    pub fn get_state(&self, id: &ComponentId) -> Option<ComponentState>;

    /// Request state transition
    pub fn transition(&self, id: &ComponentId, target: ComponentState, reason: Option<String>)
        -> Result<ComponentStateEvent, StateError>;

    /// Subscribe to state change events
    pub fn subscribe(&self) -> broadcast::Receiver<ComponentStateEvent>;

    /// Wait for a component to reach a specific state
    pub async fn wait_for_state(&self, id: &ComponentId, target: ComponentState, timeout: Duration)
        -> Result<(), StateError>;

    /// Get all components in a specific state
    pub fn get_by_state(&self, state: ComponentState) -> Vec<ComponentId>;

    /// Get all components of a type
    pub fn get_by_type(&self, component_type: ComponentType) -> Vec<ComponentId>;
}
```

---

## Phase 4: Migration Strategy (Backward Compatible)

### 4.1 StrategyState Becomes Type Alias

**File: `trading-common/src/backtest/strategy/state.rs`** (MODIFY)

```rust
// Backward compatibility: StrategyState is now an alias
pub type StrategyState = ComponentState;

// StrategyStateEvent conversion
impl From<ComponentStateEvent> for StrategyStateEvent {
    fn from(event: ComponentStateEvent) -> Self {
        StrategyStateEvent {
            strategy_id: StrategyId::new(&event.component_id.instance_name),
            old_state: event.old_state,
            new_state: event.new_state,
            reason: event.reason,
            ts_event: event.ts_event,
        }
    }
}
```

### 4.2 Strategy Trait Extension (Optional StateManaged)

**File: `trading-common/src/backtest/strategy/base.rs`** (MODIFY)

Add optional component_id field and state tracking. Strategies can opt-in to StateManaged:

```rust
// Strategies that want full state management can implement StateManaged
// The default on_state_change handler already exists and continues to work
```

---

## Phase 5: Python Bridge

**File: `trading-common/src/backtest/strategy/python_bridge.rs`** (MODIFY)

```python
# Python exposure
class ComponentState:
    UNDEFINED = 0
    SET_DEFAULTS = 1
    CONFIGURE = 2
    ACTIVE = 3
    DATA_LOADED = 4
    HISTORICAL = 5
    TRANSITION = 6
    REALTIME = 7
    TERMINATED = 8
    FAULTED = 9
    FINALIZED = 10

    def is_running(self) -> bool: ...
    def is_terminal(self) -> bool: ...
    def can_process_data(self) -> bool: ...

class StateChangeEvent:
    old_state: ComponentState
    new_state: ComponentState
    reason: Optional[str]

# Usage in Python strategy:
class MyStrategy(Strategy):
    def on_state_change(self, event: StateChangeEvent):
        if event.new_state == ComponentState.SET_DEFAULTS:
            self.period = 20
        elif event.new_state == ComponentState.DATA_LOADED:
            self.sma = self.bars.register_indicator("SMA", self.period)
        elif event.new_state == ComponentState.REALTIME:
            print("Going live!")
```

---

## Phase 6: Component Integration Examples

### 6.1 Series<T> (Indicator)

```rust
impl<T: SeriesValue> StateManaged for Series<T> {
    fn on_state_change(&mut self, event: &ComponentStateEvent) {
        match event.new_state {
            ComponentState::Configure => {
                // Set warmup period from config
            }
            ComponentState::DataLoaded => {
                // Ready to receive data
            }
            ComponentState::Terminated => {
                self.reset();
            }
            _ => {}
        }
        self.state = event.new_state;
    }
}
```

### 6.2 BarsContext

```rust
impl StateManaged for BarsContext {
    fn child_ids(&self) -> Vec<ComponentId> {
        // Return IDs of registered custom series/indicators
        self.custom_series_names().iter()
            .map(|name| ComponentId::indicator(format!("{}:{}", self.symbol, name)))
            .collect()
    }
}
```

### 6.3 Data Source (IpcExchange)

```rust
impl StateManaged for IpcExchange {
    fn on_state_change(&mut self, event: &ComponentStateEvent) {
        match event.new_state {
            ComponentState::Active => {
                // Ready to receive IPC data
                IPC_CONNECTION_STATUS.set(1);
            }
            ComponentState::Faulted | ComponentState::Terminated => {
                IPC_CONNECTION_STATUS.set(0);
            }
            _ => {}
        }
    }
}
```

### 6.4 Cache (InMemoryTickCache)

```rust
impl StateManaged for InMemoryTickCache {
    fn on_state_change(&mut self, event: &ComponentStateEvent) {
        match event.new_state {
            ComponentState::Active => {
                // Cache ready to accept data
            }
            ComponentState::Terminated => {
                self.clear_all();
            }
            _ => {}
        }
    }
}
```

---

## Files Summary

### New Files

| File | Purpose |
|------|---------|
| `trading-common/src/state/mod.rs` | ComponentState, ComponentId, ComponentType |
| `trading-common/src/state/managed.rs` | StateManaged trait |
| `trading-common/src/state/registry.rs` | StateRegistry for state queries |
| `trading-common/src/state/events.rs` | ComponentStateEvent |
| `trading-common/src/state/errors.rs` | StateError types |

### Modified Files

| File | Changes |
|------|---------|
| `trading-common/src/lib.rs` | Add `pub mod state;` export |
| `trading-common/src/backtest/strategy/state.rs` | `StrategyState` becomes type alias to `ComponentState` |
| `trading-common/src/backtest/strategy/python_bridge.rs` | Add `PyComponentState`, `PyStateChangeEvent` |
| `trading-common/src/series/mod.rs` | (Optional) Implement `StateManaged` for `Series<T>` |
| `trading-common/src/series/bars_context.rs` | (Optional) Implement `StateManaged` for `BarsContext` |
| `trading-common/src/data/cache.rs` | (Optional) Implement `StateManaged` for cache types |

---

## State Transition Rules

```
Undefined → SetDefaults
SetDefaults → Configure
Configure → Active | DataLoaded | Faulted
Active → Terminated | Faulted
DataLoaded → Historical | Faulted
Historical → Transition | Terminated | Faulted
Transition → Realtime | Faulted
Realtime → Terminated | Faulted
Terminated → Finalized
Finalized → (terminal)
Faulted → (terminal)
```

**Data-processing components** (strategies, indicators): Undefined → SetDefaults → Configure → DataLoaded → Historical → Transition → Realtime → Terminated → Finalized

**Non-data components** (services, adapters, caches): Undefined → SetDefaults → Configure → Active → Terminated → Finalized

---

## Verification

### Unit Tests

```bash
# Core state types
cargo test --package trading-common state::tests

# StateManaged trait tests
cargo test --package trading-common state::managed::tests

# Registry tests
cargo test --package trading-common state::registry::tests

# Backward compatibility
cargo test --package trading-common backtest::strategy::state::tests
```

### Integration Tests

1. **Strategy Lifecycle**: Verify strategy progresses through all states correctly
2. **Component Query**: Test that components can query other components' states via registry
3. **Hierarchical Propagation**: Test parent state changes propagate to children
4. **Python Bridge**: Verify Python strategies receive correct state events

### Manual Verification

```bash
# Run existing tests to ensure backward compatibility
cargo test --package trading-common

# Verify Python strategies work with new state system
cd trading-core
cargo run backtest --strategy python:my_strategy
```

---

## Implementation Order

1. **Phase 1**: Create `state/` module with core types (non-breaking)
2. **Phase 2**: Add `StateManaged` trait (non-breaking)
3. **Phase 3**: Create `StateRegistry` (non-breaking)
4. **Phase 4**: Make `StrategyState` a type alias (backward compatible)
5. **Phase 5**: Update Python bridge with new types
6. **Phase 6**: Gradually implement `StateManaged` for other components (opt-in)

---

## Future Enhancements (Out of Scope)

1. **Persistent State**: Save/restore component states across restarts
2. **State Visualization**: Dashboard showing all component states
3. **State Metrics**: Prometheus metrics for state transitions
4. **Dependency Graph**: Automatic state propagation based on declared dependencies
