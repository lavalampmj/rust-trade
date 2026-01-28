# src-tauri

Desktop application backend exposing trading-common functionality to Next.js frontend via Tauri IPC.

## Module Overview

```
src-tauri/src/
├── commands.rs      # Tauri command handlers
├── lib.rs           # App setup and state
└── main.rs          # Entry point
```

## Tauri Commands (commands.rs)

Exposed to frontend via `#[tauri::command]`:

| Command | Description |
|---------|-------------|
| `get_data_info()` | Backtest metadata (symbols, date ranges, record counts) |
| `get_available_strategies()` | List strategies with descriptions |
| `run_backtest()` | Execute backtest with parameters |
| `get_ohlc_preview()` | Generate candle data for charts |
| `validate_backtest_config()` | Check data availability |

## State Management

`AppState` wraps `Arc<TickDataRepository>` for thread-safe sharing:

```rust
pub struct AppState {
    pub repository: Arc<TickDataRepository>,
}

#[tauri::command]
async fn get_data_info(state: State<'_, AppState>) -> Result<DataInfo, String> {
    state.repository.get_data_info().await.map_err(|e| e.to_string())
}
```

## Development

```bash
cd frontend

# Dev mode with hot reload
npm run tauri dev

# Production build
npm run tauri build
```

## Frontend Integration

Commands are called from Next.js via `@tauri-apps/api`:

```typescript
import { invoke } from '@tauri-apps/api/tauri';

const strategies = await invoke<Strategy[]>('get_available_strategies');
const result = await invoke<BacktestResult>('run_backtest', { config });
```
