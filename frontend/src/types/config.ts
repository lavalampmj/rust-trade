/**
 * Configuration types for the trading system.
 *
 * These types mirror the Rust configuration schema and are used for
 * UI CRUD operations via Tauri commands.
 */

// ============================================================================
// Main Configuration Types
// ============================================================================

/**
 * Root application configuration.
 */
export interface AppConfig {
  /** Trading symbols to monitor */
  symbols: string[];
  /** Account configuration */
  accounts: AccountsConfig;
  /** Paper trading settings */
  paper_trading: PaperTradingConfig;
  /** Built-in strategy parameters */
  builtin_strategies: BuiltinStrategiesConfig;
  /** Python strategy configuration */
  strategies: StrategiesConfig;
  /** Execution simulation settings */
  execution: ExecutionConfig;
  /** Database settings */
  database: DatabaseConfig;
  /** Cache settings */
  cache: CacheConfig;
  /** Alerting settings */
  alerting: AlertingConfig;
  /** Service settings */
  service: ServiceConfig;
  /** Backtest settings */
  backtest: BacktestConfig;
  /** Validation settings */
  validation: ValidationSettingsConfig;
}

// ============================================================================
// Account Types
// ============================================================================

export interface AccountsConfig {
  /** Default account (used when no account_id specified) */
  default: DefaultAccountConfig;
  /** Additional simulation accounts */
  simulation: SimulationAccountConfig[];
}

export interface DefaultAccountConfig {
  /** Account identifier */
  id: string;
  /** Base currency (e.g., "USDT") */
  currency: string;
  /** Initial balance as string (decimal precision) */
  initial_balance: string;
}

export interface SimulationAccountConfig {
  /** Account identifier */
  id: string;
  /** Base currency */
  currency: string;
  /** Initial balance as string */
  initial_balance: string;
  /** Strategy IDs that use this account */
  strategies: string[];
}

// ============================================================================
// Paper Trading Types
// ============================================================================

export interface PaperTradingConfig {
  /** Enable paper trading */
  enabled: boolean;
  /** Strategy to use for paper trading */
  strategy: string;
  /** Initial capital for paper trading */
  initial_capital: number;
}

// ============================================================================
// Strategy Types
// ============================================================================

export interface BuiltinStrategiesConfig {
  /** RSI strategy parameters */
  rsi: RsiStrategyConfig;
  /** SMA strategy parameters */
  sma: SmaStrategyConfig;
}

export interface RsiStrategyConfig {
  /** RSI calculation period */
  period: number;
  /** Oversold threshold (buy signal) */
  oversold: number;
  /** Overbought threshold (sell signal) */
  overbought: number;
  /** Order quantity */
  order_quantity: string;
  /** Lookback multiplier */
  lookback_multiplier: number;
  /** Lookback buffer */
  lookback_buffer: number;
}

export interface SmaStrategyConfig {
  /** Short period moving average */
  short_period: number;
  /** Long period moving average */
  long_period: number;
  /** Order quantity */
  order_quantity: string;
  /** Lookback multiplier */
  lookback_multiplier: number;
}

export interface StrategiesConfig {
  /** Directory containing Python strategy files */
  python_dir: string;
  /** Enable hot-reload for development */
  hot_reload: boolean;
  /** Hot-reload configuration */
  hot_reload_config: HotReloadConfig;
  /** Registered Python strategies */
  python: PythonStrategyConfig[];
}

export interface HotReloadConfig {
  /** Debounce time in milliseconds */
  debounce_ms: number;
  /** Skip hash verification (development only) */
  skip_hash_verification: boolean;
}

export interface PythonStrategyConfig {
  /** Strategy identifier */
  id: string;
  /** Python file path (relative to python_dir) */
  file: string;
  /** Python class name */
  class_name: string;
  /** Strategy description */
  description: string;
  /** Enable/disable strategy */
  enabled: boolean;
  /** SHA256 hash for verification (computed on backend) */
  sha256: string;
  /** Execution limits */
  limits: PythonStrategyLimits;
}

export interface PythonStrategyLimits {
  /** Maximum execution time per tick in milliseconds */
  max_execution_time_ms: number;
}

// ============================================================================
// Execution Types
// ============================================================================

export interface ExecutionConfig {
  /** Fee configuration */
  fees: FeesConfig;
  /** Latency simulation */
  latency: LatencyConfig;
}

export interface FeesConfig {
  /** Default fee model: "zero", "binance_spot", "binance_futures", "custom" */
  default_model: FeeModel;
  /** Binance spot fees */
  binance_spot: FeeRate;
  /** Binance spot with BNB discount */
  binance_spot_bnb: FeeRate;
  /** Binance futures fees */
  binance_futures: FeeRate;
  /** Custom fee rates */
  custom: FeeRate;
}

export type FeeModel =
  | "zero"
  | "binance_spot"
  | "binance_spot_bnb"
  | "binance_futures"
  | "custom";

export interface FeeRate {
  /** Maker fee rate */
  maker: string;
  /** Taker fee rate */
  taker: string;
}

export interface LatencyConfig {
  /** Latency model: "none", "fixed", "variable" */
  model: LatencyModel;
  /** Order insert latency in milliseconds */
  insert_ms: number;
  /** Order update latency in milliseconds */
  update_ms: number;
  /** Order delete latency in milliseconds */
  delete_ms: number;
  /** Jitter for variable latency */
  jitter_ms: number;
  /** Random seed (0 = random each run) */
  seed: number;
}

export type LatencyModel = "none" | "fixed" | "variable";

// ============================================================================
// Infrastructure Types
// ============================================================================

export interface DatabaseConfig {
  /** Maximum database connections */
  max_connections: number;
  /** Minimum database connections */
  min_connections: number;
  /** Connection max lifetime in seconds */
  max_lifetime: number;
  /** Connection acquire timeout in seconds */
  acquire_timeout_secs: number;
  /** Idle connection timeout in seconds */
  idle_timeout_secs: number;
}

export interface CacheConfig {
  /** In-memory cache settings */
  memory: MemoryCacheConfig;
  /** Redis cache settings */
  redis: RedisCacheConfig;
}

export interface MemoryCacheConfig {
  /** Maximum ticks per symbol */
  max_ticks_per_symbol: number;
  /** TTL in seconds */
  ttl_seconds: number;
}

export interface RedisCacheConfig {
  /** Connection pool size */
  pool_size: number;
  /** TTL in seconds */
  ttl_seconds: number;
  /** Maximum ticks per symbol */
  max_ticks_per_symbol: number;
}

export interface AlertingConfig {
  /** Enable alerting system */
  enabled: boolean;
  /** Evaluation interval in seconds */
  interval_secs: number;
  /** Cooldown between repeated alerts in seconds */
  cooldown_secs: number;
  /** Pool saturation warning threshold (0.0-1.0) */
  pool_saturation_threshold: number;
  /** Pool critical threshold (0.0-1.0) */
  pool_critical_threshold: number;
  /** Batch failure rate threshold (0.0-1.0) */
  batch_failure_threshold: number;
  /** Channel backpressure threshold (0.0-100.0) */
  channel_backpressure_threshold: number;
  /** Reconnection storm threshold */
  reconnection_storm_threshold: number;
}

export interface ServiceConfig {
  /** Channel capacity for tick processing */
  channel_capacity: number;
  /** Health monitor interval in seconds */
  health_monitor_interval_secs: number;
  /** Bar timer interval in seconds */
  bar_timer_interval_secs: number;
  /** Reconnection delay in seconds */
  reconnection_delay_secs: number;
}

export interface BacktestConfig {
  /** Default commission rate */
  default_commission_rate: string;
  /** Default number of records */
  default_records: number;
  /** Default initial capital */
  default_capital: string;
  /** Minimum records required */
  minimum_records: number;
}

export interface ValidationSettingsConfig {
  /** Enable tick validation */
  enabled: boolean;
  /** Maximum price change percentage */
  max_price_change_pct: number;
  /** Timestamp tolerance in minutes */
  timestamp_tolerance_minutes: number;
  /** Maximum past days for timestamps */
  max_past_days: number;
}

// ============================================================================
// API Response Types
// ============================================================================

/**
 * Response from get_config command.
 */
export interface ConfigResponse {
  /** The merged configuration */
  config: AppConfig;
  /** Whether there are user overrides */
  has_overrides: boolean;
}

/**
 * Validation result from validate_config commands.
 */
export interface ValidationResult {
  /** Whether the configuration is valid */
  valid: boolean;
  /** List of validation errors */
  errors: ValidationError[];
  /** List of validation warnings */
  warnings: ValidationWarning[];
}

export interface ValidationError {
  /** Field path */
  field: string;
  /** Error message */
  message: string;
  /** Error code */
  code: string;
}

export interface ValidationWarning {
  /** Field path */
  field: string;
  /** Warning message */
  message: string;
}

/**
 * Audit log entry for configuration changes.
 */
export interface ConfigAuditEntry {
  /** Timestamp */
  timestamp: string;
  /** Section that was changed */
  section: string;
  /** Action performed: "update", "add", "remove", "reset" */
  action: ConfigAction;
  /** Previous value */
  old_value?: unknown;
  /** New value */
  new_value?: unknown;
}

export type ConfigAction = "update" | "add" | "remove" | "reset";

// ============================================================================
// Symbol Presets
// ============================================================================

/**
 * Preset symbol lists for quick selection.
 */
export const SYMBOL_PRESETS = {
  TOP_10: [
    "BTCUSDT",
    "ETHUSDT",
    "SOLUSDT",
    "BNBUSDT",
    "XRPUSDT",
    "TRXUSDT",
    "ADAUSDT",
    "DOGEUSDT",
    "AVAXUSDT",
    "DOTUSDT",
  ],
  MAJORS: ["BTCUSDT", "ETHUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT"],
  STABLECOINS: ["USDTUSDC", "BUSDUSDT", "DAIUSDT"],
  DEFI: [
    "UNIUSDT",
    "AAVEUSDT",
    "LINKUSDT",
    "MKRUSDT",
    "COMPUSDT",
    "SUSHIUSDT",
  ],
} as const;

// ============================================================================
// Fee Model Options
// ============================================================================

export const FEE_MODEL_OPTIONS: Array<{ value: FeeModel; label: string }> = [
  { value: "zero", label: "Zero Fees" },
  { value: "binance_spot", label: "Binance Spot (0.1%)" },
  { value: "binance_spot_bnb", label: "Binance Spot + BNB (0.075%)" },
  { value: "binance_futures", label: "Binance Futures (0.02%/0.04%)" },
  { value: "custom", label: "Custom Fees" },
];

export const LATENCY_MODEL_OPTIONS: Array<{
  value: LatencyModel;
  label: string;
}> = [
  { value: "none", label: "No Latency" },
  { value: "fixed", label: "Fixed Latency" },
  { value: "variable", label: "Variable Latency" },
];
