# UI Configuration Management Plan

This document outlines the plan for enabling user configuration management through a Tauri + Next.js desktop UI.

## Table of Contents

1. [TOML vs JSON Analysis](#toml-vs-json-analysis)
2. [Configuration Inventory](#configuration-inventory)
3. [CRUD API Design](#crud-api-design)
4. [Implementation Plan](#implementation-plan)
5. [Security Considerations](#security-considerations)

---

## TOML vs JSON Analysis

### Current State: TOML

The codebase currently uses TOML for all configuration:

| File | Purpose |
|------|---------|
| `config/development.toml` | Main trading-core configuration |
| `config/production.toml` | Production overrides |
| `config/test.toml` | Test environment config |
| `data-manager/config/default.toml` | Data manager defaults |
| `data-manager/config/development.toml` | Data manager dev overrides |

### Comparison

| Aspect | TOML | JSON |
|--------|------|------|
| **Human Readability** | Excellent - comments, hierarchical sections | Good - no comments, requires formatting |
| **Rust Ecosystem** | Native with `config` crate, `serde` support | Native `serde_json`, universal |
| **Comments** | ✅ Supported | ❌ Not supported (need workarounds) |
| **TypeScript/JS** | Requires parser library | Native `JSON.parse/stringify` |
| **CRUD Operations** | Complex - preserve comments/formatting | Simple - native object manipulation |
| **Schema Validation** | Manual or custom | JSON Schema standard |
| **Editor Support** | Good (TOML plugins) | Excellent (native everywhere) |

### Recommendation: Hybrid Approach

**Keep TOML for source control, use JSON for runtime CRUD.**

```
┌─────────────────────────────────────────────────────────────────┐
│                     Configuration Flow                          │
│                                                                 │
│  TOML Files          JSON Config Store         Runtime Config   │
│  (git tracked)  ───► (user customizations) ───► (merged)       │
│                                                                 │
│  development.toml    user_config.json          AppConfig       │
│  (defaults)          (CRUD operations)         (final values)  │
└─────────────────────────────────────────────────────────────────┘
```

**Rationale:**
1. TOML files remain as documented defaults (with comments)
2. User customizations stored in JSON (easy CRUD from UI)
3. Runtime merges: `defaults (TOML) + user_config (JSON) = final config`
4. JSON changes don't require restart if hot-reload is implemented

---

## Configuration Inventory

### User-Configurable Settings (UI Exposure Priority)

#### Priority 1: Essential Trading Settings

| Section | Settings | UI Widget Type |
|---------|----------|----------------|
| **Trading Pairs** | `symbols[]` | Multi-select with search, drag-to-reorder |
| **Paper Trading** | `paper_trading.enabled`, `strategy`, `initial_capital` | Toggle, dropdown, currency input |
| **Default Account** | `accounts.default.*` | Form (id, currency, balance) |

#### Priority 2: Strategy Configuration

| Section | Settings | UI Widget Type |
|---------|----------|----------------|
| **RSI Strategy** | `period`, `oversold`, `overbought`, `order_quantity` | Sliders, number inputs |
| **SMA Strategy** | `short_period`, `long_period`, `order_quantity` | Number inputs |
| **Python Strategies** | `[[strategies.python]]` array | List with add/edit/delete modals |

#### Priority 3: Execution & Fees

| Section | Settings | UI Widget Type |
|---------|----------|----------------|
| **Fee Model** | `execution.fees.default_model` | Dropdown (zero/binance_spot/custom) |
| **Custom Fees** | `execution.fees.custom.*` | Decimal inputs (maker/taker) |
| **Latency Simulation** | `execution.latency.*` | Number inputs, toggle |

#### Priority 4: Infrastructure (Advanced)

| Section | Settings | UI Widget Type |
|---------|----------|----------------|
| **Database** | `database.max_connections` | Number input |
| **Cache** | `cache.memory.*`, `cache.redis.*` | Number inputs |
| **IPC** | `ipc.*` | Read-only display or advanced toggle |
| **Alerting** | `alerting.*` | Toggle, threshold inputs |

#### Priority 5: Data Manager Settings

| Section | Settings | UI Widget Type |
|---------|----------|----------------|
| **Binance Provider** | `provider.binance.*` | URL input, reconnection settings |
| **Backfill** | `backfill.*` | Complex form with cost limits |
| **Validation** | `validation.*` | Threshold inputs |

### Read-Only/System Settings (No UI Exposure)

These should **not** be editable via UI:

- `server.host`, `server.port` - Security risk
- `database.url` - Contains credentials
- `metrics.bind_address` - System-level
- `strategies.python[].sha256` - Security hashes
- `transport.ipc.shm_path_prefix` - System paths

---

## CRUD API Design

### Tauri Command Interface

All configuration CRUD operations go through Tauri commands defined in `src-tauri/src/commands.rs`.

```rust
// src-tauri/src/commands/config.rs

/// Get the merged configuration (defaults + user overrides)
#[tauri::command]
pub async fn get_config(
    state: State<'_, AppState>,
) -> Result<ConfigResponse, String>;

/// Get a specific configuration section
#[tauri::command]
pub async fn get_config_section(
    section: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String>;

/// Update a configuration section
#[tauri::command]
pub async fn update_config_section(
    section: String,
    value: serde_json::Value,
    state: State<'_, AppState>,
) -> Result<(), String>;

/// Reset a section to defaults
#[tauri::command]
pub async fn reset_config_section(
    section: String,
    state: State<'_, AppState>,
) -> Result<(), String>;

/// Validate configuration before applying
#[tauri::command]
pub async fn validate_config(
    config: serde_json::Value,
) -> Result<ValidationResult, String>;
```

### REST-like Section Mapping

| HTTP-like Operation | Tauri Command | Section Path |
|---------------------|---------------|--------------|
| GET /config | `get_config` | Full config |
| GET /config/symbols | `get_config_section("symbols")` | Trading pairs |
| PUT /config/symbols | `update_config_section("symbols", [...])` | Update pairs |
| GET /config/accounts | `get_config_section("accounts")` | Account settings |
| PUT /config/accounts/default | `update_config_section("accounts.default", {...})` | Update default account |
| POST /config/accounts/simulation | `add_config_item("accounts.simulation", {...})` | Add simulation account |
| DELETE /config/accounts/simulation/0 | `remove_config_item("accounts.simulation", 0)` | Remove by index |
| GET /config/strategies/python | `get_config_section("strategies.python")` | Python strategies |
| POST /config/strategies/python | `add_config_item("strategies.python", {...})` | Add strategy |
| PUT /config/strategies/python/0 | `update_config_item("strategies.python", 0, {...})` | Update strategy |
| DELETE /config/strategies/python/0 | `remove_config_item("strategies.python", 0)` | Remove strategy |

### TypeScript API Client

```typescript
// frontend/src/lib/config-api.ts

export interface ConfigAPI {
  // Full config
  getConfig(): Promise<AppConfig>;

  // Section operations
  getSection<T>(section: string): Promise<T>;
  updateSection<T>(section: string, value: T): Promise<void>;
  resetSection(section: string): Promise<void>;

  // Array item operations (for strategies, accounts, symbols)
  addItem<T>(section: string, item: T): Promise<void>;
  updateItem<T>(section: string, index: number, item: T): Promise<void>;
  removeItem(section: string, index: number): Promise<void>;

  // Validation
  validateConfig(config: Partial<AppConfig>): Promise<ValidationResult>;
}

// Implementation using Tauri invoke
export const configApi: ConfigAPI = {
  getConfig: () => invoke('get_config'),
  getSection: (section) => invoke('get_config_section', { section }),
  updateSection: (section, value) => invoke('update_config_section', { section, value }),
  // ... etc
};
```

### Data Types (Shared)

```typescript
// frontend/src/types/config.ts

export interface TradingSymbol {
  symbol: string;
  enabled: boolean;
}

export interface AccountConfig {
  id: string;
  currency: string;
  initial_balance: string;  // Decimal as string
}

export interface SimulationAccount extends AccountConfig {
  strategies?: string[];
}

export interface PythonStrategy {
  id: string;
  file: string;
  class_name: string;
  description: string;
  enabled: boolean;
  sha256: string;
  limits?: {
    max_execution_time_ms: number;
  };
}

export interface FeeConfig {
  maker: string;  // Decimal as string
  taker: string;
}

export interface StrategyParams {
  rsi: {
    period: number;
    oversold: number;
    overbought: number;
    order_quantity: string;
  };
  sma: {
    short_period: number;
    long_period: number;
    order_quantity: string;
  };
}
```

---

## Implementation Plan

### Phase 1: Config Service Layer (Backend)

**Duration: 1-2 days**

1. Create `trading-common/src/config/service.rs`:
   - `ConfigService` struct to manage config lifecycle
   - Load defaults from TOML
   - Load user overrides from JSON
   - Merge with precedence: user > defaults
   - Validation before save

2. Create user config storage:
   - Location: `~/.config/rust-trade/user_config.json`
   - Or Tauri app data directory

3. Add Tauri commands in `src-tauri/src/commands/config.rs`:
   - Implement all CRUD operations
   - Wire to ConfigService

```
trading-common/src/
├── config/
│   ├── mod.rs
│   ├── service.rs      # NEW: ConfigService
│   ├── schema.rs       # NEW: Config types with serde
│   └── validation.rs   # NEW: Validation rules
```

### Phase 2: Settings UI Scaffolding (Frontend)

**Duration: 2-3 days**

1. Create settings page structure:
   ```
   frontend/src/app/settings/
   ├── page.tsx              # Main settings page
   ├── layout.tsx            # Settings layout with sidebar
   ├── trading/page.tsx      # Trading pairs
   ├── accounts/page.tsx     # Account management
   ├── strategies/page.tsx   # Strategy configuration
   ├── execution/page.tsx    # Fees & latency
   └── advanced/page.tsx     # Infrastructure settings
   ```

2. Components:
   ```
   frontend/src/components/settings/
   ├── SymbolSelector.tsx    # Multi-select for trading pairs
   ├── AccountForm.tsx       # Account CRUD form
   ├── StrategyCard.tsx      # Strategy config card
   ├── FeeModelSelector.tsx  # Fee model dropdown
   └── SliderInput.tsx       # Numeric slider with input
   ```

### Phase 3: Trading Pairs UI

**Duration: 1 day**

- Symbol multi-select with search
- Drag-and-drop reordering
- Enable/disable toggle per symbol
- Preset buttons (Top 10, Majors, Stablecoins)

### Phase 4: Account Management UI

**Duration: 1-2 days**

- Default account form
- Simulation accounts list (add/edit/delete)
- Strategy-to-account mapping
- Balance validation

### Phase 5: Strategy Configuration UI

**Duration: 2-3 days**

- Built-in strategy parameter editors (RSI, SMA sliders)
- Python strategy registration wizard
- File picker for .py files
- Auto-hash calculation
- Enable/disable toggle
- Execution timeout settings

### Phase 6: Execution Settings UI

**Duration: 1 day**

- Fee model selector (dropdown)
- Custom fee inputs
- Latency simulation toggles
- Visual feedback on expected costs

### Phase 7: Hot Reload Integration

**Duration: 1-2 days**

- Config change detection
- Apply changes without restart where possible
- Restart prompt for changes requiring restart
- Undo/redo support

### Phase 8: Data Manager UI (Optional)

**Duration: 2-3 days**

- Backfill cost calculator
- Provider configuration
- Routing rules editor
- Validation threshold editor

---

## Security Considerations

### Sensitive Fields

Never expose via UI or API:
- Database URLs/credentials
- API keys (Databento, Binance)
- SHA256 hashes (write-only, compute on backend)
- System paths

### Input Validation

| Field Type | Validation |
|------------|------------|
| Decimal (balance, fees) | Regex: `^\d+(\.\d+)?$`, range check |
| Symbol | Uppercase alphanumeric, length 3-20 |
| Strategy ID | Lowercase alphanumeric + underscore |
| File paths | Restrict to allowed directories |
| Periods | Integer > 0, max reasonable value |

### Rate Limiting

- Config save: Max 1 per second
- Prevent rapid-fire updates from UI bugs

### Audit Log

Consider logging config changes:
```json
{
  "timestamp": "2026-01-24T10:30:00Z",
  "section": "accounts.default",
  "action": "update",
  "old_value": {...},
  "new_value": {...}
}
```

---

## File Structure Summary

```
rust-trade/
├── config/
│   ├── development.toml       # Defaults (git tracked)
│   ├── production.toml
│   └── schema.json            # NEW: JSON Schema for validation
├── trading-common/src/
│   └── config/
│       ├── service.rs         # NEW: ConfigService
│       ├── schema.rs          # NEW: Type definitions
│       └── validation.rs      # NEW: Validation
├── src-tauri/src/
│   └── commands/
│       └── config.rs          # NEW: Config CRUD commands
├── frontend/src/
│   ├── app/settings/          # NEW: Settings pages
│   ├── components/settings/   # NEW: Settings components
│   ├── lib/config-api.ts      # NEW: Config API client
│   └── types/config.ts        # NEW: TypeScript types
└── ~/.config/rust-trade/
    └── user_config.json       # User customizations (not git tracked)
```

---

## Next Steps

1. **Immediate**: Review this plan and prioritize phases
2. **Decision needed**: Confirm hybrid TOML+JSON approach
3. **First task**: Implement `ConfigService` in trading-common
4. **Parallel**: Start frontend scaffolding while backend develops

---

*Document created: 2026-01-24*
*Last updated: 2026-01-24*
