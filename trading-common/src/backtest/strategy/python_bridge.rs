use crate::accounts::AccountEvent;
use crate::data::events::MarketDataEvent;
use crate::data::types::{BarData, BarDataMode, BarType, Timeframe, TradeSide};
use crate::instruments::SessionEvent;
use crate::orders::{
    ClientOrderId, Order, OrderCanceled, OrderEventAny, OrderFilled, OrderRejected, OrderSide,
    TimeInForce,
};
use crate::series::bars_context::BarsContext;
use crate::series::MaximumBarsLookBack;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
// OHLCData included via BarData.ohlc_bar
use super::base::Strategy;
use super::position::PositionEvent;
use super::state::StrategyStateEvent;
use rust_decimal::Decimal;
use std::str::FromStr;

/// Error types that can occur in the Python bridge
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum PythonBridgeError {
    /// Error converting data between Rust and Python
    ConversionError(String),
    /// Error calling Python method
    MethodCallError(String),
    /// Python strategy raised an exception
    PythonException(String),
    /// Strategy is disabled due to too many errors
    CircuitBreakerOpen,
    /// GIL acquisition timeout
    GilTimeout,
}

impl std::fmt::Display for PythonBridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PythonBridgeError::ConversionError(msg) => write!(f, "Conversion error: {}", msg),
            PythonBridgeError::MethodCallError(msg) => write!(f, "Method call error: {}", msg),
            PythonBridgeError::PythonException(msg) => write!(f, "Python exception: {}", msg),
            PythonBridgeError::CircuitBreakerOpen => {
                write!(f, "Circuit breaker open - strategy disabled")
            }
            PythonBridgeError::GilTimeout => write!(f, "GIL acquisition timeout"),
        }
    }
}

impl std::error::Error for PythonBridgeError {}

/// Callback type for error notifications
pub type ErrorCallback = Box<dyn Fn(&PythonBridgeError, &str) + Send + Sync>;

/// Configuration for error recovery behavior
#[derive(Debug, Clone)]
pub struct ErrorRecoveryConfig {
    /// Maximum consecutive errors before circuit breaker opens
    pub max_consecutive_errors: u32,
    /// Whether to automatically reset error count on success
    pub reset_on_success: bool,
    /// Whether to log errors to stderr
    pub log_errors: bool,
    /// Whether circuit breaker is enabled
    pub circuit_breaker_enabled: bool,
}

impl Default for ErrorRecoveryConfig {
    fn default() -> Self {
        Self {
            max_consecutive_errors: 10,
            reset_on_success: true,
            log_errors: true,
            circuit_breaker_enabled: true,
        }
    }
}

/// Error statistics for monitoring
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct ErrorStats {
    /// Total errors since creation
    pub total_errors: u64,
    /// Consecutive errors (resets on success)
    pub consecutive_errors: u32,
    /// Conversion errors
    pub conversion_errors: u64,
    /// Method call errors
    pub method_errors: u64,
    /// Python exceptions
    pub python_exceptions: u64,
    /// Last error message
    pub last_error: Option<String>,
    /// Timestamp of last error
    pub last_error_time: Option<chrono::DateTime<chrono::Utc>>,
}

/// Wrapper around Python strategy instance
pub struct PythonStrategy {
    /// Python object instance (stored as PyObject to avoid GIL issues)
    py_instance: Arc<Mutex<PyObject>>,
    /// Python BarsContext instance for OHLCV series
    py_bars_context: Arc<Mutex<PyObject>>,
    /// Cached strategy name
    cached_name: String,

    // Resource tracking (thread-safe)
    /// Total CPU time spent in microseconds
    cpu_time_us: Arc<AtomicU64>,
    /// Total number of on_bar_data calls
    call_count: Arc<AtomicU64>,
    /// Peak execution time in microseconds
    peak_execution_us: Arc<AtomicU64>,

    // Error recovery (thread-safe)
    /// Error recovery configuration
    error_config: ErrorRecoveryConfig,
    /// Error statistics
    error_stats: Arc<RwLock<ErrorStats>>,
    /// Consecutive error count (atomic for fast access)
    consecutive_errors: Arc<AtomicU64>,
    /// Circuit breaker state (true = open/disabled)
    circuit_open: Arc<AtomicBool>,
    /// Error callback (optional)
    error_callback: Arc<RwLock<Option<ErrorCallback>>>,
}

impl std::fmt::Debug for PythonStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PythonStrategy")
            .field("cached_name", &self.cached_name)
            .field("cpu_time_us", &self.cpu_time_us.load(Ordering::Relaxed))
            .field("call_count", &self.call_count.load(Ordering::Relaxed))
            .field(
                "peak_execution_us",
                &self.peak_execution_us.load(Ordering::Relaxed),
            )
            .field(
                "consecutive_errors",
                &self.consecutive_errors.load(Ordering::Relaxed),
            )
            .field("circuit_open", &self.circuit_open.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl PythonStrategy {
    /// Load strategy from Python file
    pub fn from_file(path: &str, class_name: &str) -> Result<Self, String> {
        Python::with_gil(|py| {
            // Add strategy directory to Python path
            let sys = py
                .import_bound("sys")
                .map_err(|e| format!("Failed to import sys: {}", e))?;
            let py_path = sys
                .getattr("path")
                .map_err(|e| format!("Failed to get sys.path: {}", e))?;

            // Add parent directory of strategy file to path
            let strategy_path = std::path::Path::new(path);
            if let Some(parent) = strategy_path.parent() {
                py_path
                    .call_method1("insert", (0, parent.to_str().unwrap()))
                    .map_err(|e| format!("Failed to add to sys.path: {}", e))?;

                // Also add strategies root (for _lib package imports)
                // If parent is "examples" or "_tests", go up one more level
                if let Some(parent_name) = parent.file_name().and_then(|n| n.to_str()) {
                    if parent_name == "examples" || parent_name == "_tests" {
                        if let Some(strategies_root) = parent.parent() {
                            py_path
                                .call_method1("insert", (0, strategies_root.to_str().unwrap()))
                                .map_err(|e| {
                                    format!("Failed to add strategies root to sys.path: {}", e)
                                })?;

                            // Add _lib directory for restricted_compiler import
                            let lib_path = strategies_root.join("_lib");
                            if lib_path.exists() {
                                py_path
                                    .call_method1("insert", (0, lib_path.to_str().unwrap()))
                                    .map_err(|e| {
                                        format!("Failed to add _lib to sys.path: {}", e)
                                    })?;
                            }
                        }
                    }
                }
            }

            // Load the Python module from file
            let code = std::fs::read_to_string(path)
                .map_err(|e| format!("Failed to read strategy file: {}", e))?;

            let module_name = std::path::Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or("Invalid file name")?;

            // Import restricted compiler module
            let restricted_compiler = py.import_bound("restricted_compiler").map_err(|e| {
                format!(
                    "Failed to import restricted_compiler: {}\n\
                     Make sure RestrictedPython is installed: pip install RestrictedPython==7.0\n\
                     And strategies/_lib/restricted_compiler.py exists",
                    e
                )
            })?;

            // Compile strategy code with restrictions
            let result = restricted_compiler
                .getattr("compile_strategy")
                .map_err(|e| format!("Failed to get compile_strategy function: {}", e))?
                .call1((code.as_str(), path))
                .map_err(|e| {
                    format!(
                        "Failed to compile strategy with restrictions: {}\n\
                     This usually means the strategy code violates security policies.\n\
                     Check for: prohibited imports (os, subprocess, urllib, socket, sys), \n\
                     eval/exec usage, or unsafe operations.",
                        e
                    )
                })?;

            // Extract bytecode and globals from tuple
            let tuple = result
                .downcast::<PyTuple>()
                .map_err(|e| format!("compile_strategy should return tuple: {}", e))?;
            let bytecode = tuple
                .get_item(0)
                .map_err(|e| format!("Failed to get bytecode from tuple: {}", e))?;
            let globals_item = tuple
                .get_item(1)
                .map_err(|e| format!("Failed to get globals from tuple: {}", e))?;
            let restricted_globals = globals_item
                .downcast::<PyDict>()
                .map_err(|e| format!("restricted_globals should be dict: {}", e))?;

            // Set module metadata in restricted globals
            restricted_globals
                .set_item("__name__", module_name)
                .map_err(|e| format!("Failed to set __name__: {}", e))?;
            restricted_globals
                .set_item("__file__", path)
                .map_err(|e| format!("Failed to set __file__: {}", e))?;

            // Execute bytecode in restricted environment using Python's exec
            let builtins = py
                .import_bound("builtins")
                .map_err(|e| format!("Failed to import builtins: {}", e))?;
            let exec_fn = builtins
                .getattr("exec")
                .map_err(|e| format!("Failed to get exec function: {}", e))?;

            exec_fn.call1((bytecode, restricted_globals)).map_err(|e| {
                format!(
                    "Failed to execute strategy code: {}\n\
                     The strategy may be trying to use prohibited operations.",
                    e
                )
            })?;

            // Get the strategy class from restricted globals
            let strategy_class = restricted_globals
                .get_item(class_name)
                .map_err(|e| format!("Failed to get item from globals: {}", e))?
                .ok_or_else(|| format!("Class '{}' not found in module", class_name))?;

            // Instantiate the strategy
            let instance = strategy_class
                .call0()
                .map_err(|e| format!("Failed to instantiate strategy: {}", e))?;

            // Cache metadata
            let name = instance
                .call_method0("name")
                .and_then(|n: Bound<'_, PyAny>| n.extract::<String>())
                .map_err(|e| format!("Failed to get strategy name: {}", e))?;

            // Get max_bars_lookback from strategy (default 256)
            let max_lookback = match instance.call_method0("max_bars_lookback") {
                Ok(result) => result.extract::<usize>().unwrap_or(256),
                Err(_) => 256,
            };

            // Import and create Python BarsContext
            let bars_module = py
                .import_bound("_lib.bars_context")
                .map_err(|e| format!("Failed to import _lib.bars_context: {}", e))?;
            let bars_class = bars_module
                .getattr("BarsContext")
                .map_err(|e| format!("Failed to get BarsContext class: {}", e))?;

            let py_bars = bars_class
                .call1(("", max_lookback))
                .map_err(|e| format!("Failed to create BarsContext: {}", e))?;

            Ok(Self {
                py_instance: Arc::new(Mutex::new(instance.into())),
                py_bars_context: Arc::new(Mutex::new(py_bars.into())),
                cached_name: name,
                cpu_time_us: Arc::new(AtomicU64::new(0)),
                call_count: Arc::new(AtomicU64::new(0)),
                peak_execution_us: Arc::new(AtomicU64::new(0)),
                error_config: ErrorRecoveryConfig::default(),
                error_stats: Arc::new(RwLock::new(ErrorStats::default())),
                consecutive_errors: Arc::new(AtomicU64::new(0)),
                circuit_open: Arc::new(AtomicBool::new(false)),
                error_callback: Arc::new(RwLock::new(None)),
            })
        })
    }

    /// Create a new PythonStrategy with custom error recovery config
    #[allow(dead_code)]
    pub fn from_file_with_config(
        path: &str,
        class_name: &str,
        error_config: ErrorRecoveryConfig,
    ) -> Result<Self, String> {
        let mut strategy = Self::from_file(path, class_name)?;
        strategy.error_config = error_config;
        Ok(strategy)
    }

    /// Set the error callback for monitoring
    #[allow(dead_code)]
    pub fn set_error_callback(&self, callback: ErrorCallback) {
        if let Ok(mut cb) = self.error_callback.write() {
            *cb = Some(callback);
        }
    }

    /// Get current error statistics
    #[allow(dead_code)]
    pub fn get_error_stats(&self) -> ErrorStats {
        self.error_stats
            .read()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// Check if circuit breaker is open (strategy disabled)
    #[allow(dead_code)]
    pub fn is_circuit_open(&self) -> bool {
        self.circuit_open.load(Ordering::Relaxed)
    }

    /// Manually reset the circuit breaker
    #[allow(dead_code)]
    pub fn reset_circuit_breaker(&self) {
        self.circuit_open.store(false, Ordering::Relaxed);
        self.consecutive_errors.store(0, Ordering::Relaxed);
        if let Ok(mut stats) = self.error_stats.write() {
            stats.consecutive_errors = 0;
        }
    }

    /// Record an error and potentially open circuit breaker
    fn record_error(&self, error: PythonBridgeError, context: &str) {
        // Update statistics
        let consecutive = self.consecutive_errors.fetch_add(1, Ordering::Relaxed) + 1;

        if let Ok(mut stats) = self.error_stats.write() {
            stats.total_errors += 1;
            stats.consecutive_errors = consecutive as u32;
            stats.last_error = Some(error.to_string());
            stats.last_error_time = Some(chrono::Utc::now());

            match &error {
                PythonBridgeError::ConversionError(_) => stats.conversion_errors += 1,
                PythonBridgeError::MethodCallError(_) | PythonBridgeError::PythonException(_) => {
                    stats.method_errors += 1;
                }
                _ => {}
            }
        }

        // Log if enabled
        if self.error_config.log_errors {
            eprintln!(
                "[PythonBridge] {} error in {}: {}",
                self.cached_name, context, error
            );
        }

        // Notify callback
        if let Ok(callback) = self.error_callback.read() {
            if let Some(ref cb) = *callback {
                cb(&error, context);
            }
        }

        // Check circuit breaker threshold
        if self.error_config.circuit_breaker_enabled
            && consecutive as u32 >= self.error_config.max_consecutive_errors
        {
            self.circuit_open.store(true, Ordering::Relaxed);
            if self.error_config.log_errors {
                eprintln!(
                    "[PythonBridge] Circuit breaker OPEN for {} after {} consecutive errors",
                    self.cached_name, consecutive
                );
            }
        }
    }

    /// Record a successful operation (resets consecutive error count)
    fn record_success(&self) {
        if self.error_config.reset_on_success {
            let prev = self.consecutive_errors.swap(0, Ordering::Relaxed);
            if prev > 0 {
                if let Ok(mut stats) = self.error_stats.write() {
                    stats.consecutive_errors = 0;
                }
            }
        }
    }

    /// Get peak execution time in microseconds
    pub fn get_peak_execution_us(&self) -> u64 {
        self.peak_execution_us.load(Ordering::Relaxed)
    }

    /// Get average execution time in microseconds
    pub fn get_avg_execution_us(&self) -> u64 {
        let total = self.cpu_time_us.load(Ordering::Relaxed);
        let count = self.call_count.load(Ordering::Relaxed);
        if count > 0 {
            total / count
        } else {
            0
        }
    }
}

/// Test-only methods for inspecting internal metrics
#[cfg(test)]
impl PythonStrategy {
    /// Get total CPU time spent in microseconds (test only)
    pub fn get_cpu_time_us(&self) -> u64 {
        self.cpu_time_us.load(Ordering::Relaxed)
    }

    /// Get total number of calls (test only)
    pub fn get_call_count(&self) -> u64 {
        self.call_count.load(Ordering::Relaxed)
    }

    /// Reset resource tracking metrics (test only)
    pub fn reset_metrics(&self) {
        self.cpu_time_us.store(0, Ordering::Relaxed);
        self.call_count.store(0, Ordering::Relaxed);
        self.peak_execution_us.store(0, Ordering::Relaxed);
    }

    /// Get consecutive error count (test only)
    pub fn get_consecutive_errors(&self) -> u64 {
        self.consecutive_errors.load(Ordering::Relaxed)
    }

    /// Simulate an error for testing (test only)
    pub fn simulate_error(&self, error: PythonBridgeError, context: &str) {
        self.record_error(error, context);
    }

    /// Set error config for testing (test only)
    pub fn set_error_config(&mut self, config: ErrorRecoveryConfig) {
        self.error_config = config;
    }

    /// Reset error tracking (test only)
    pub fn reset_error_stats(&self) {
        self.consecutive_errors.store(0, Ordering::Relaxed);
        self.circuit_open.store(false, Ordering::Relaxed);
        if let Ok(mut stats) = self.error_stats.write() {
            *stats = ErrorStats::default();
        }
    }
}

/// Convert Rust BarData to Python dict
fn bar_data_to_pydict<'a>(py: Python<'a>, bar_data: &BarData) -> PyResult<Bound<'a, PyDict>> {
    let dict = PyDict::new_bound(py);

    // Add OHLC bar data
    let ohlc = &bar_data.ohlc_bar;
    dict.set_item("timestamp", ohlc.timestamp.to_rfc3339())?;
    dict.set_item("symbol", &ohlc.symbol)?;
    dict.set_item("open", ohlc.open.to_string())?;
    dict.set_item("high", ohlc.high.to_string())?;
    dict.set_item("low", ohlc.low.to_string())?;
    dict.set_item("close", ohlc.close.to_string())?;
    dict.set_item("volume", ohlc.volume.to_string())?;
    dict.set_item("trade_count", ohlc.trade_count)?;

    // Add metadata
    dict.set_item(
        "is_first_tick_of_bar",
        bar_data.metadata.is_first_tick_of_bar,
    )?;
    dict.set_item("is_bar_closed", bar_data.metadata.is_bar_closed)?;
    dict.set_item("is_synthetic", bar_data.metadata.is_synthetic)?;
    dict.set_item("tick_count_in_bar", bar_data.metadata.tick_count_in_bar)?;

    // Add current tick if present
    if let Some(ref tick) = bar_data.current_tick {
        let tick_dict = PyDict::new_bound(py);
        tick_dict.set_item("timestamp", tick.timestamp.to_rfc3339())?;
        tick_dict.set_item("price", tick.price.to_string())?;
        tick_dict.set_item("quantity", tick.quantity.to_string())?;
        tick_dict.set_item(
            "side",
            match tick.side {
                TradeSide::Buy => "Buy",
                TradeSide::Sell => "Sell",
            },
        )?;
        tick_dict.set_item("trade_id", &tick.trade_id)?;
        dict.set_item("current_tick", tick_dict)?;
    } else {
        dict.set_item("current_tick", py.None())?;
    }

    Ok(dict)
}

/// Parse timeframe string to Timeframe enum
fn parse_timeframe(s: &str) -> Option<Timeframe> {
    match s {
        "1m" => Some(Timeframe::OneMinute),
        "5m" => Some(Timeframe::FiveMinutes),
        "15m" => Some(Timeframe::FifteenMinutes),
        "30m" => Some(Timeframe::ThirtyMinutes),
        "1h" => Some(Timeframe::OneHour),
        "4h" => Some(Timeframe::FourHours),
        "1d" => Some(Timeframe::OneDay),
        "1w" => Some(Timeframe::OneWeek),
        _ => None,
    }
}

/// Convert Python order dict to Rust Order
fn pydict_to_order(dict: &Bound<'_, PyDict>) -> PyResult<Order> {
    // Required fields
    let symbol = dict
        .get_item("symbol")?
        .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err("Missing 'symbol'"))?
        .extract::<String>()?;

    let order_side_str = dict
        .get_item("order_side")?
        .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err("Missing 'order_side'"))?
        .extract::<String>()?;
    let order_side = match order_side_str.as_str() {
        "Buy" => OrderSide::Buy,
        "Sell" => OrderSide::Sell,
        _ => {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Invalid order_side: {}",
                order_side_str
            )))
        }
    };

    let order_type_str = dict
        .get_item("order_type")?
        .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err("Missing 'order_type'"))?
        .extract::<String>()?;

    let quantity_str = dict
        .get_item("quantity")?
        .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err("Missing 'quantity'"))?
        .extract::<String>()?;
    let quantity = Decimal::from_str(&quantity_str)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid quantity: {}", e)))?;

    // Optional fields
    let price: Option<Decimal> = dict
        .get_item("price")?
        .and_then(|v| v.extract::<String>().ok())
        .and_then(|s| Decimal::from_str(&s).ok());

    let stop_price: Option<Decimal> = dict
        .get_item("stop_price")?
        .and_then(|v| v.extract::<String>().ok())
        .and_then(|s| Decimal::from_str(&s).ok());

    let time_in_force_str: String = dict
        .get_item("time_in_force")?
        .and_then(|v| v.extract::<String>().ok())
        .unwrap_or_else(|| "GTC".to_string());
    let time_in_force = match time_in_force_str.as_str() {
        "GTC" => TimeInForce::GTC,
        "IOC" => TimeInForce::IOC,
        "FOK" => TimeInForce::FOK,
        "DAY" => TimeInForce::Day,
        "GTD" => TimeInForce::GTD,
        _ => TimeInForce::GTC,
    };

    let client_order_id: Option<String> = dict
        .get_item("client_order_id")?
        .and_then(|v| v.extract::<String>().ok());

    // Build order based on type
    let mut builder = match order_type_str.as_str() {
        "Market" => Order::market(&symbol, order_side, quantity),
        "Limit" => {
            let price = price.ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err("Limit orders require 'price'")
            })?;
            Order::limit(&symbol, order_side, quantity, price)
        }
        "Stop" | "StopMarket" => {
            let trigger = stop_price.ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err("Stop orders require 'stop_price'")
            })?;
            Order::stop(&symbol, order_side, quantity, trigger)
        }
        "StopLimit" => {
            let price = price.ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err("StopLimit orders require 'price'")
            })?;
            let trigger = stop_price.ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err("StopLimit orders require 'stop_price'")
            })?;
            Order::stop_limit(&symbol, order_side, quantity, price, trigger)
        }
        _ => {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Unsupported order_type: {}",
                order_type_str
            )))
        }
    };

    builder = builder.with_time_in_force(time_in_force);

    if let Some(coid) = client_order_id {
        builder = builder.with_client_order_id(coid);
    }

    builder.build().map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("Failed to build order: {}", e))
    })
}

impl Strategy for PythonStrategy {
    fn name(&self) -> &str {
        &self.cached_name
    }

    fn on_bar_data(&mut self, bar_data: &BarData, _bars: &mut BarsContext) {
        // Note: Python strategies use their own Python BarsContext
        // The Rust _bars parameter is ignored; we pass Python BarsContext instead

        // Check circuit breaker first
        if self.circuit_open.load(Ordering::Relaxed) {
            return;
        }

        // Start timing
        let start = std::time::Instant::now();
        let mut had_error = false;

        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_bars = self.py_bars_context.lock().unwrap();
            let py_instance = instance.bind(py);
            let py_bars_bound = py_bars.bind(py);

            // Convert BarData to Python dict
            let bar_dict = match bar_data_to_pydict(py, bar_data) {
                Ok(d) => d,
                Err(e) => {
                    had_error = true;
                    self.record_error(
                        PythonBridgeError::ConversionError(e.to_string()),
                        "on_bar_data/bar_data_to_pydict",
                    );
                    return;
                }
            };

            // Update Python BarsContext BEFORE calling strategy
            if let Err(e) = py_bars_bound.call_method1("on_bar_update", (&bar_dict,)) {
                had_error = true;
                self.record_error(
                    PythonBridgeError::MethodCallError(e.to_string()),
                    "on_bar_data/on_bar_update",
                );
                return;
            }

            // Call Python on_bar_data method with BOTH bar_data AND bars
            // Python strategy should store orders internally and return them via get_orders()
            if let Err(e) = py_instance.call_method1("on_bar_data", (&bar_dict, py_bars_bound)) {
                had_error = true;
                self.record_error(
                    PythonBridgeError::PythonException(e.to_string()),
                    "on_bar_data",
                );
            }
        });

        // Record success if no errors
        if !had_error {
            self.record_success();
        }

        // Record execution time
        let elapsed_us = start.elapsed().as_micros() as u64;
        self.cpu_time_us.fetch_add(elapsed_us, Ordering::Relaxed);
        self.call_count.fetch_add(1, Ordering::Relaxed);

        // Update peak if necessary
        let current_peak = self.peak_execution_us.load(Ordering::Relaxed);
        if elapsed_us > current_peak {
            self.peak_execution_us.store(elapsed_us, Ordering::Relaxed);
        }

        // Warn if execution is slow (>10ms)
        if elapsed_us > 10_000 {
            tracing::warn!(
                "Strategy {} on_bar_data took {}ms (peak: {}ms, avg: {}ms)",
                self.cached_name,
                elapsed_us / 1000,
                self.get_peak_execution_us() / 1000,
                self.get_avg_execution_us() / 1000
            );
        }
    }

    fn initialize(&mut self, params: HashMap<String, String>) -> Result<(), String> {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            // Convert HashMap to Python dict
            let py_params = PyDict::new_bound(py);
            for (k, v) in params.iter() {
                py_params
                    .set_item(k, v)
                    .map_err(|e| format!("Failed to set param: {}", e))?;
            }

            // Call initialize
            let result = py_instance
                .call_method1("initialize", (&py_params,))
                .map_err(|e| format!("Python initialize error: {}", e))?;

            // Check if error message returned
            if let Ok(Some(err_msg)) = result.extract::<Option<String>>() {
                return Err(err_msg);
            }

            Ok(())
        })
    }

    fn reset(&mut self) {
        Python::with_gil(|py| {
            // Reset strategy
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            if let Err(e) = py_instance.call_method0("reset") {
                eprintln!("Python reset error: {}", e);
            }

            // Reset BarsContext
            let py_bars = self.py_bars_context.lock().unwrap();
            if let Err(e) = py_bars.bind(py).call_method0("reset") {
                eprintln!("Python BarsContext reset error: {}", e);
            }
        });
    }

    fn bar_data_mode(&self) -> BarDataMode {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            match py_instance.call_method0("bar_data_mode") {
                Ok(result) => {
                    if let Ok(mode_str) = result.extract::<String>() {
                        match mode_str.as_str() {
                            "OnEachTick" => BarDataMode::OnEachTick,
                            "OnPriceMove" => BarDataMode::OnPriceMove,
                            "OnCloseBar" => BarDataMode::OnCloseBar,
                            _ => BarDataMode::OnCloseBar,
                        }
                    } else {
                        BarDataMode::OnCloseBar
                    }
                }
                Err(_) => BarDataMode::OnCloseBar,
            }
        })
    }

    fn preferred_bar_type(&self) -> BarType {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            match py_instance.call_method0("preferred_bar_type") {
                Ok(result) => {
                    if let Ok(bar_type_dict) = result.downcast::<PyDict>() {
                        if let Ok(Some(type_str)) = bar_type_dict.get_item("type") {
                            if let Ok(type_name) = type_str.extract::<String>() {
                                match type_name.as_str() {
                                    "TimeBased" => {
                                        if let Ok(Some(tf)) = bar_type_dict.get_item("timeframe") {
                                            if let Ok(tf_str) = tf.extract::<String>() {
                                                if let Some(timeframe) = parse_timeframe(&tf_str) {
                                                    return BarType::TimeBased(timeframe);
                                                }
                                            }
                                        }
                                        BarType::TimeBased(Timeframe::OneMinute)
                                    }
                                    "TickBased" => {
                                        if let Ok(Some(count)) =
                                            bar_type_dict.get_item("tick_count")
                                        {
                                            if let Ok(n) = count.extract::<u32>() {
                                                return BarType::TickBased(n);
                                            }
                                        }
                                        BarType::TickBased(100)
                                    }
                                    _ => BarType::TimeBased(Timeframe::OneMinute),
                                }
                            } else {
                                BarType::TimeBased(Timeframe::OneMinute)
                            }
                        } else {
                            BarType::TimeBased(Timeframe::OneMinute)
                        }
                    } else {
                        BarType::TimeBased(Timeframe::OneMinute)
                    }
                }
                Err(_) => BarType::TimeBased(Timeframe::OneMinute),
            }
        })
    }

    fn max_bars_lookback(&self) -> MaximumBarsLookBack {
        // Python strategies manage their own state
        // Use default lookback for consistency
        MaximumBarsLookBack::Fixed(256)
    }

    fn is_ready(&self, _bars: &BarsContext) -> bool {
        // Call Python strategy's is_ready method with Python BarsContext
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_bars = self.py_bars_context.lock().unwrap();
            let py_instance = instance.bind(py);
            let py_bars_bound = py_bars.bind(py);

            match py_instance.call_method1("is_ready", (py_bars_bound,)) {
                Ok(result) => result.extract::<bool>().unwrap_or(true),
                Err(e) => {
                    // Log error and default to ready (fail-open for backwards compatibility)
                    eprintln!("Python is_ready error: {} - defaulting to true", e);
                    true
                }
            }
        })
    }

    fn warmup_period(&self) -> usize {
        // Call Python strategy's warmup_period method
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            match py_instance.call_method0("warmup_period") {
                Ok(result) => result.extract::<usize>().unwrap_or(0),
                Err(e) => {
                    // Log error and default to 0 (no warmup required)
                    eprintln!("Python warmup_period error: {} - defaulting to 0", e);
                    0
                }
            }
        })
    }

    // ========================================================================
    // Order Event Handlers
    // ========================================================================

    fn on_order_filled(&mut self, event: &OrderFilled) {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            let event_dict = PyDict::new_bound(py);
            let _ = event_dict.set_item("client_order_id", event.client_order_id.as_str());
            let _ = event_dict.set_item("venue_order_id", event.venue_order_id.as_str());
            let _ = event_dict.set_item("symbol", event.instrument_id.symbol.as_str());
            let _ = event_dict.set_item(
                "order_side",
                match event.order_side {
                    OrderSide::Buy => "Buy",
                    OrderSide::Sell => "Sell",
                },
            );
            let _ = event_dict.set_item("last_qty", event.last_qty.to_string());
            let _ = event_dict.set_item("last_px", event.last_px.to_string());
            let _ = event_dict.set_item("cum_qty", event.cum_qty.to_string());
            let _ = event_dict.set_item("leaves_qty", event.leaves_qty.to_string());
            let _ = event_dict.set_item("commission", event.commission.to_string());
            let _ = event_dict.set_item("timestamp", event.ts_event.to_rfc3339());

            if let Err(e) = py_instance.call_method1("on_order_filled", (&event_dict,)) {
                eprintln!("Python on_order_filled error: {}", e);
            }
        });
    }

    fn on_order_rejected(&mut self, event: &OrderRejected) {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            let event_dict = PyDict::new_bound(py);
            let _ = event_dict.set_item("client_order_id", event.client_order_id.as_str());
            let _ = event_dict.set_item("reason", &event.reason);
            let _ = event_dict.set_item("timestamp", event.ts_event.to_rfc3339());

            if let Err(e) = py_instance.call_method1("on_order_rejected", (&event_dict,)) {
                eprintln!("Python on_order_rejected error: {}", e);
            }
        });
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            let event_dict = PyDict::new_bound(py);
            let _ = event_dict.set_item("client_order_id", event.client_order_id.as_str());
            if let Some(ref venue_id) = event.venue_order_id {
                let _ = event_dict.set_item("venue_order_id", venue_id.as_str());
            }
            let _ = event_dict.set_item("timestamp", event.ts_event.to_rfc3339());

            if let Err(e) = py_instance.call_method1("on_order_canceled", (&event_dict,)) {
                eprintln!("Python on_order_canceled error: {}", e);
            }
        });
    }

    fn on_order_submitted(&mut self, order: &Order) {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            let order_dict = PyDict::new_bound(py);
            let _ = order_dict.set_item("client_order_id", order.client_order_id.as_str());
            let _ = order_dict.set_item("symbol", order.instrument_id.symbol.as_str());
            let _ = order_dict.set_item(
                "order_side",
                match order.side {
                    OrderSide::Buy => "Buy",
                    OrderSide::Sell => "Sell",
                },
            );
            let _ = order_dict.set_item("order_type", order.order_type.to_string());
            let _ = order_dict.set_item("quantity", order.quantity.to_string());
            if let Some(price) = order.price {
                let _ = order_dict.set_item("price", price.to_string());
            }
            if let Some(trigger_price) = order.trigger_price {
                let _ = order_dict.set_item("stop_price", trigger_price.to_string());
            }
            let _ = order_dict.set_item("time_in_force", order.time_in_force.to_string());

            if let Err(e) = py_instance.call_method1("on_order_submitted", (&order_dict,)) {
                eprintln!("Python on_order_submitted error: {}", e);
            }
        });
    }

    fn get_orders(&mut self, bar_data: &BarData, _bars: &mut BarsContext) -> Vec<Order> {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_bars = self.py_bars_context.lock().unwrap();
            let py_instance = instance.bind(py);
            let py_bars_bound = py_bars.bind(py);

            let bar_dict = match bar_data_to_pydict(py, bar_data) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("Failed to convert BarData for get_orders: {}", e);
                    return Vec::new();
                }
            };

            match py_instance.call_method1("get_orders", (&bar_dict, py_bars_bound)) {
                Ok(result) => {
                    if let Ok(orders_list) = result.extract::<Vec<Bound<'_, PyDict>>>() {
                        orders_list
                            .iter()
                            .filter_map(|order_dict| pydict_to_order(order_dict).ok())
                            .collect()
                    } else {
                        Vec::new()
                    }
                }
                Err(e) => {
                    eprintln!("Python get_orders error: {}", e);
                    Vec::new()
                }
            }
        })
    }

    fn get_cancellations(
        &mut self,
        bar_data: &BarData,
        _bars: &mut BarsContext,
    ) -> Vec<ClientOrderId> {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_bars = self.py_bars_context.lock().unwrap();
            let py_instance = instance.bind(py);
            let py_bars_bound = py_bars.bind(py);

            let bar_dict = match bar_data_to_pydict(py, bar_data) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("Failed to convert BarData for get_cancellations: {}", e);
                    return Vec::new();
                }
            };

            match py_instance.call_method1("get_cancellations", (&bar_dict, py_bars_bound)) {
                Ok(result) => {
                    if let Ok(ids) = result.extract::<Vec<String>>() {
                        ids.into_iter().map(|s| ClientOrderId::new(s)).collect()
                    } else {
                        Vec::new()
                    }
                }
                Err(e) => {
                    eprintln!("Python get_cancellations error: {}", e);
                    Vec::new()
                }
            }
        })
    }

    // ========================================================================
    // Event-Driven Handlers (NinjaTrader-style)
    // ========================================================================

    fn on_order_update(&mut self, event: &OrderEventAny) {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            // Check if method exists
            if !py_instance.hasattr("on_order_update").unwrap_or(false) {
                return;
            }

            let event_dict = PyDict::new_bound(py);

            // Add event type
            let event_type = match event {
                OrderEventAny::Initialized(_) => "Initialized",
                OrderEventAny::Denied(_) => "Denied",
                OrderEventAny::Submitted(_) => "Submitted",
                OrderEventAny::Accepted(_) => "Accepted",
                OrderEventAny::Rejected(_) => "Rejected",
                OrderEventAny::Canceled(_) => "Canceled",
                OrderEventAny::Expired(_) => "Expired",
                OrderEventAny::Triggered(_) => "Triggered",
                OrderEventAny::PendingUpdate(_) => "PendingUpdate",
                OrderEventAny::PendingCancel(_) => "PendingCancel",
                OrderEventAny::ModifyRejected(_) => "ModifyRejected",
                OrderEventAny::CancelRejected(_) => "CancelRejected",
                OrderEventAny::Updated(_) => "Updated",
                OrderEventAny::Filled(_) => "Filled",
            };
            let _ = event_dict.set_item("event_type", event_type);

            // Add common fields based on event type
            match event {
                OrderEventAny::Submitted(e) => {
                    let _ = event_dict.set_item("client_order_id", e.client_order_id.as_str());
                    let _ = event_dict.set_item("timestamp", e.ts_event.to_rfc3339());
                }
                OrderEventAny::Accepted(e) => {
                    let _ = event_dict.set_item("client_order_id", e.client_order_id.as_str());
                    let _ = event_dict.set_item("venue_order_id", e.venue_order_id.as_str());
                    let _ = event_dict.set_item("timestamp", e.ts_event.to_rfc3339());
                }
                OrderEventAny::Rejected(e) => {
                    let _ = event_dict.set_item("client_order_id", e.client_order_id.as_str());
                    let _ = event_dict.set_item("reason", &e.reason);
                    let _ = event_dict.set_item("timestamp", e.ts_event.to_rfc3339());
                }
                OrderEventAny::Canceled(e) => {
                    let _ = event_dict.set_item("client_order_id", e.client_order_id.as_str());
                    if let Some(ref venue_id) = e.venue_order_id {
                        let _ = event_dict.set_item("venue_order_id", venue_id.as_str());
                    }
                    let _ = event_dict.set_item("timestamp", e.ts_event.to_rfc3339());
                }
                OrderEventAny::Filled(e) => {
                    let _ = event_dict.set_item("client_order_id", e.client_order_id.as_str());
                    let _ = event_dict.set_item("venue_order_id", e.venue_order_id.as_str());
                    let _ = event_dict.set_item("symbol", e.instrument_id.symbol.as_str());
                    let _ = event_dict.set_item(
                        "order_side",
                        match e.order_side {
                            OrderSide::Buy => "Buy",
                            OrderSide::Sell => "Sell",
                        },
                    );
                    let _ = event_dict.set_item("last_qty", e.last_qty.to_string());
                    let _ = event_dict.set_item("last_px", e.last_px.to_string());
                    let _ = event_dict.set_item("cum_qty", e.cum_qty.to_string());
                    let _ = event_dict.set_item("leaves_qty", e.leaves_qty.to_string());
                    let _ = event_dict.set_item("is_last_fill", e.leaves_qty.is_zero());
                    let _ = event_dict.set_item("timestamp", e.ts_event.to_rfc3339());
                }
                _ => {
                    // For other event types, just add client_order_id and timestamp
                    let _ =
                        event_dict.set_item("client_order_id", event.client_order_id().as_str());
                    let _ = event_dict.set_item("timestamp", event.ts_event().to_rfc3339());
                }
            }

            if let Err(e) = py_instance.call_method1("on_order_update", (&event_dict,)) {
                eprintln!("Python on_order_update error: {}", e);
            }
        });
    }

    fn on_execution(&mut self, event: &OrderFilled) {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            // Check if method exists
            if !py_instance.hasattr("on_execution").unwrap_or(false) {
                return;
            }

            let event_dict = PyDict::new_bound(py);
            let _ = event_dict.set_item("client_order_id", event.client_order_id.as_str());
            let _ = event_dict.set_item("venue_order_id", event.venue_order_id.as_str());
            let _ = event_dict.set_item("symbol", event.instrument_id.symbol.as_str());
            let _ = event_dict.set_item(
                "order_side",
                match event.order_side {
                    OrderSide::Buy => "Buy",
                    OrderSide::Sell => "Sell",
                },
            );
            let _ = event_dict.set_item("last_qty", event.last_qty.to_string());
            let _ = event_dict.set_item("last_px", event.last_px.to_string());
            let _ = event_dict.set_item("cum_qty", event.cum_qty.to_string());
            let _ = event_dict.set_item("leaves_qty", event.leaves_qty.to_string());
            let _ = event_dict.set_item("commission", event.commission.to_string());
            let _ = event_dict.set_item("commission_currency", &event.commission_currency);
            let _ = event_dict.set_item("is_last_fill", event.leaves_qty.is_zero());
            let _ = event_dict.set_item("timestamp", event.ts_event.to_rfc3339());

            if let Err(e) = py_instance.call_method1("on_execution", (&event_dict,)) {
                eprintln!("Python on_execution error: {}", e);
            }
        });
    }

    fn on_position_update(&mut self, event: &PositionEvent) {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            // Check if method exists
            if !py_instance.hasattr("on_position_update").unwrap_or(false) {
                return;
            }

            let event_dict = PyDict::new_bound(py);
            let _ = event_dict.set_item("position_id", event.position_id.as_str());
            let _ = event_dict.set_item("account_id", event.account_id.as_str());
            let _ = event_dict.set_item("symbol", event.symbol());
            let _ = event_dict.set_item("strategy_id", event.strategy_id.as_str());
            let _ = event_dict.set_item("side", format!("{:?}", event.side));
            let _ = event_dict.set_item("quantity", event.quantity.to_string());
            let _ = event_dict.set_item("avg_entry_price", event.avg_entry_price.to_string());
            let _ = event_dict.set_item("unrealized_pnl", event.unrealized_pnl.to_string());
            let _ = event_dict.set_item("realized_pnl", event.realized_pnl.to_string());
            let _ = event_dict.set_item("commission", event.commission.to_string());
            let _ = event_dict.set_item("mark_price", event.mark_price.to_string());
            let _ = event_dict.set_item("notional", event.notional().to_string());
            let _ = event_dict.set_item("total_pnl", event.total_pnl().to_string());
            let _ = event_dict.set_item("net_pnl", event.net_pnl().to_string());
            let _ = event_dict.set_item("is_open", event.is_open());
            let _ = event_dict.set_item("is_flat", event.is_flat());
            let _ = event_dict.set_item("is_long", event.is_long());
            let _ = event_dict.set_item("is_short", event.is_short());
            let _ = event_dict.set_item("ts_opened", event.ts_opened.to_rfc3339());
            let _ = event_dict.set_item("timestamp", event.ts_event.to_rfc3339());

            if let Err(e) = py_instance.call_method1("on_position_update", (&event_dict,)) {
                eprintln!("Python on_position_update error: {}", e);
            }
        });
    }

    fn on_account_update(&mut self, event: &AccountEvent) {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            // Check if method exists
            if !py_instance.hasattr("on_account_update").unwrap_or(false) {
                return;
            }

            let event_dict = PyDict::new_bound(py);

            match event {
                AccountEvent::Created {
                    account_id,
                    timestamp,
                } => {
                    let _ = event_dict.set_item("event_type", "Created");
                    let _ = event_dict.set_item("account_id", account_id.as_str());
                    let _ = event_dict.set_item("timestamp", timestamp.to_rfc3339());
                }
                AccountEvent::BalanceUpdated {
                    account_id,
                    currency,
                    old_balance,
                    new_balance,
                    timestamp,
                } => {
                    let _ = event_dict.set_item("event_type", "BalanceUpdated");
                    let _ = event_dict.set_item("account_id", account_id.as_str());
                    let _ = event_dict.set_item("currency", currency);
                    let _ = event_dict.set_item("old_total", old_balance.total.to_string());
                    let _ = event_dict.set_item("old_free", old_balance.free.to_string());
                    let _ = event_dict.set_item("new_total", new_balance.total.to_string());
                    let _ = event_dict.set_item("new_free", new_balance.free.to_string());
                    let _ = event_dict.set_item("timestamp", timestamp.to_rfc3339());
                }
                AccountEvent::StateChanged {
                    account_id,
                    old_state,
                    new_state,
                    timestamp,
                } => {
                    let _ = event_dict.set_item("event_type", "StateChanged");
                    let _ = event_dict.set_item("account_id", account_id.as_str());
                    let _ = event_dict.set_item("old_state", format!("{:?}", old_state));
                    let _ = event_dict.set_item("new_state", format!("{:?}", new_state));
                    let _ = event_dict.set_item("timestamp", timestamp.to_rfc3339());
                }
                AccountEvent::MarginUpdated {
                    account_id,
                    margin_used,
                    margin_available,
                    unrealized_pnl,
                    timestamp,
                } => {
                    let _ = event_dict.set_item("event_type", "MarginUpdated");
                    let _ = event_dict.set_item("account_id", account_id.as_str());
                    let _ = event_dict.set_item("margin_used", margin_used.to_string());
                    let _ = event_dict.set_item("margin_available", margin_available.to_string());
                    let _ = event_dict.set_item("unrealized_pnl", unrealized_pnl.to_string());
                    let _ = event_dict.set_item("timestamp", timestamp.to_rfc3339());
                }
            }

            if let Err(e) = py_instance.call_method1("on_account_update", (&event_dict,)) {
                eprintln!("Python on_account_update error: {}", e);
            }
        });
    }

    fn on_marketdata_update(&mut self, event: &MarketDataEvent) {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            // Check if method exists
            if !py_instance.hasattr("on_marketdata_update").unwrap_or(false) {
                return;
            }

            let event_dict = PyDict::new_bound(py);
            let _ = event_dict.set_item("data_type", format!("{:?}", event.data_type));
            let _ = event_dict.set_item("symbol", event.instrument_id.symbol.as_str());
            let _ = event_dict.set_item("timestamp", event.ts_event.to_rfc3339());

            // Add tick data if present
            if let Some(ref tick) = event.tick {
                let tick_dict = PyDict::new_bound(py);
                let _ = tick_dict.set_item("price", tick.price.to_string());
                let _ = tick_dict.set_item("quantity", tick.quantity.to_string());
                let _ = tick_dict.set_item(
                    "side",
                    match tick.side {
                        TradeSide::Buy => "Buy",
                        TradeSide::Sell => "Sell",
                    },
                );
                let _ = tick_dict.set_item("trade_id", &tick.trade_id);
                let _ = tick_dict.set_item("timestamp", tick.timestamp.to_rfc3339());
                let _ = event_dict.set_item("tick", tick_dict);
            }

            // Add quote data if present
            if let Some(ref quote) = event.quote {
                let quote_dict = PyDict::new_bound(py);
                let _ = quote_dict.set_item("bid_price", quote.bid_price.to_string());
                let _ = quote_dict.set_item("bid_size", quote.bid_size.to_string());
                let _ = quote_dict.set_item("ask_price", quote.ask_price.to_string());
                let _ = quote_dict.set_item("ask_size", quote.ask_size.to_string());
                let _ = quote_dict.set_item("timestamp", quote.ts_event.to_rfc3339());
                let _ = event_dict.set_item("quote", quote_dict);
            }

            if let Err(e) = py_instance.call_method1("on_marketdata_update", (&event_dict,)) {
                eprintln!("Python on_marketdata_update error: {}", e);
            }
        });
    }

    fn on_state_change(&mut self, event: &StrategyStateEvent) {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            // Check if method exists
            if !py_instance.hasattr("on_state_change").unwrap_or(false) {
                return;
            }

            let event_dict = PyDict::new_bound(py);
            let _ = event_dict.set_item("strategy_id", event.strategy_id.as_str());

            // String representations for display/debugging
            let _ = event_dict.set_item("old_state", format!("{}", event.old_state));
            let _ = event_dict.set_item("new_state", format!("{}", event.new_state));

            // Integer values for programmatic use (matches ComponentState::as_i32())
            // Python code can compare: if event['new_state_int'] == ComponentState.REALTIME
            let _ = event_dict.set_item("old_state_int", event.old_state.as_i32());
            let _ = event_dict.set_item("new_state_int", event.new_state.as_i32());

            // Helper booleans for common state checks
            let _ = event_dict.set_item("is_going_live", event.is_going_live());
            let _ = event_dict.set_item("is_terminal", event.is_terminal_transition());
            let _ = event_dict.set_item("is_fault", event.is_fault());
            let _ = event_dict.set_item("can_process_data", event.new_state.can_process_data());
            let _ = event_dict.set_item("is_running", event.new_state.is_running());

            if let Some(ref reason) = event.reason {
                let _ = event_dict.set_item("reason", reason);
            }
            let _ = event_dict.set_item("timestamp", event.ts_event.to_rfc3339());

            if let Err(e) = py_instance.call_method1("on_state_change", (&event_dict,)) {
                eprintln!("Python on_state_change error: {}", e);
            }
        });
    }

    fn on_session_update(&mut self, event: &SessionEvent) {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            // Check if method exists
            if !py_instance.hasattr("on_session_update").unwrap_or(false) {
                return;
            }

            let event_dict = PyDict::new_bound(py);

            match event {
                SessionEvent::SessionOpened { symbol, session } => {
                    let _ = event_dict.set_item("event_type", "SessionOpened");
                    let _ = event_dict.set_item("symbol", symbol);
                    let _ = event_dict.set_item("session_name", &session.name);
                    let _ =
                        event_dict.set_item("session_type", format!("{:?}", session.session_type));
                }
                SessionEvent::SessionClosed { symbol } => {
                    let _ = event_dict.set_item("event_type", "SessionClosed");
                    let _ = event_dict.set_item("symbol", symbol);
                }
                SessionEvent::MarketHalted { symbol, reason } => {
                    let _ = event_dict.set_item("event_type", "MarketHalted");
                    let _ = event_dict.set_item("symbol", symbol);
                    let _ = event_dict.set_item("reason", reason);
                }
                SessionEvent::MarketResumed { symbol } => {
                    let _ = event_dict.set_item("event_type", "MarketResumed");
                    let _ = event_dict.set_item("symbol", symbol);
                }
                SessionEvent::MaintenanceStarted { symbol } => {
                    let _ = event_dict.set_item("event_type", "MaintenanceStarted");
                    let _ = event_dict.set_item("symbol", symbol);
                }
                SessionEvent::MaintenanceEnded { symbol } => {
                    let _ = event_dict.set_item("event_type", "MaintenanceEnded");
                    let _ = event_dict.set_item("symbol", symbol);
                }
            }

            if let Err(e) = py_instance.call_method1("on_session_update", (&event_dict,)) {
                eprintln!("Python on_session_update error: {}", e);
            }
        });
    }
}

// Thread safety: PythonStrategy is Send + Sync because:
// 1. PyObject is wrapped in Arc<Mutex<>>
// 2. All Python access is guarded by GIL via Python::with_gil()
// 3. Cached fields are immutable after construction
unsafe impl Send for PythonStrategy {}
unsafe impl Sync for PythonStrategy {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{BarMetadata, OHLCData, TickData};
    use chrono::Utc;
    use rust_decimal_macros::dec;

    // ========================================================================
    // parse_timeframe tests
    // ========================================================================

    #[test]
    fn test_parse_timeframe_valid() {
        assert_eq!(parse_timeframe("1m"), Some(Timeframe::OneMinute));
        assert_eq!(parse_timeframe("5m"), Some(Timeframe::FiveMinutes));
        assert_eq!(parse_timeframe("15m"), Some(Timeframe::FifteenMinutes));
        assert_eq!(parse_timeframe("30m"), Some(Timeframe::ThirtyMinutes));
        assert_eq!(parse_timeframe("1h"), Some(Timeframe::OneHour));
        assert_eq!(parse_timeframe("4h"), Some(Timeframe::FourHours));
        assert_eq!(parse_timeframe("1d"), Some(Timeframe::OneDay));
        assert_eq!(parse_timeframe("1w"), Some(Timeframe::OneWeek));
    }

    #[test]
    fn test_parse_timeframe_invalid() {
        assert_eq!(parse_timeframe(""), None);
        assert_eq!(parse_timeframe("invalid"), None);
        assert_eq!(parse_timeframe("2m"), None);
        assert_eq!(parse_timeframe("1M"), None); // Case sensitive
        assert_eq!(parse_timeframe("1H"), None);
        assert_eq!(parse_timeframe("1D"), None);
    }

    // ========================================================================
    // pydict_to_order tests (require Python GIL)
    // ========================================================================

    #[test]
    fn test_pydict_to_order_market() {
        Python::with_gil(|py| {
            let dict = PyDict::new_bound(py);
            dict.set_item("symbol", "BTCUSDT").unwrap();
            dict.set_item("order_side", "Buy").unwrap();
            dict.set_item("order_type", "Market").unwrap();
            dict.set_item("quantity", "0.5").unwrap();

            let order = pydict_to_order(&dict).unwrap();
            assert_eq!(order.instrument_id.symbol, "BTCUSDT");
            assert_eq!(order.side, OrderSide::Buy);
            assert_eq!(order.quantity, dec!(0.5));
            assert_eq!(order.time_in_force, TimeInForce::GTC);
        });
    }

    #[test]
    fn test_pydict_to_order_limit() {
        Python::with_gil(|py| {
            let dict = PyDict::new_bound(py);
            dict.set_item("symbol", "ETHUSDT").unwrap();
            dict.set_item("order_side", "Sell").unwrap();
            dict.set_item("order_type", "Limit").unwrap();
            dict.set_item("quantity", "2.0").unwrap();
            dict.set_item("price", "3500.00").unwrap();

            let order = pydict_to_order(&dict).unwrap();
            assert_eq!(order.instrument_id.symbol, "ETHUSDT");
            assert_eq!(order.side, OrderSide::Sell);
            assert_eq!(order.quantity, dec!(2.0));
            assert_eq!(order.price, Some(dec!(3500.00)));
        });
    }

    #[test]
    fn test_pydict_to_order_limit_missing_price() {
        Python::with_gil(|py| {
            let dict = PyDict::new_bound(py);
            dict.set_item("symbol", "BTCUSDT").unwrap();
            dict.set_item("order_side", "Buy").unwrap();
            dict.set_item("order_type", "Limit").unwrap();
            dict.set_item("quantity", "1.0").unwrap();
            // Missing price

            let result = pydict_to_order(&dict);
            assert!(result.is_err());
        });
    }

    #[test]
    fn test_pydict_to_order_stop() {
        Python::with_gil(|py| {
            let dict = PyDict::new_bound(py);
            dict.set_item("symbol", "BTCUSDT").unwrap();
            dict.set_item("order_side", "Sell").unwrap();
            dict.set_item("order_type", "Stop").unwrap();
            dict.set_item("quantity", "1.0").unwrap();
            dict.set_item("stop_price", "45000.00").unwrap();

            let order = pydict_to_order(&dict).unwrap();
            assert_eq!(order.instrument_id.symbol, "BTCUSDT");
            assert_eq!(order.side, OrderSide::Sell);
            assert_eq!(order.trigger_price, Some(dec!(45000.00)));
        });
    }

    #[test]
    fn test_pydict_to_order_stop_market() {
        Python::with_gil(|py| {
            let dict = PyDict::new_bound(py);
            dict.set_item("symbol", "BTCUSDT").unwrap();
            dict.set_item("order_side", "Buy").unwrap();
            dict.set_item("order_type", "StopMarket").unwrap();
            dict.set_item("quantity", "0.1").unwrap();
            dict.set_item("stop_price", "50000.00").unwrap();

            let order = pydict_to_order(&dict).unwrap();
            assert_eq!(order.trigger_price, Some(dec!(50000.00)));
        });
    }

    #[test]
    fn test_pydict_to_order_stop_missing_trigger() {
        Python::with_gil(|py| {
            let dict = PyDict::new_bound(py);
            dict.set_item("symbol", "BTCUSDT").unwrap();
            dict.set_item("order_side", "Sell").unwrap();
            dict.set_item("order_type", "Stop").unwrap();
            dict.set_item("quantity", "1.0").unwrap();
            // Missing stop_price

            let result = pydict_to_order(&dict);
            assert!(result.is_err());
        });
    }

    #[test]
    fn test_pydict_to_order_stop_limit() {
        Python::with_gil(|py| {
            let dict = PyDict::new_bound(py);
            dict.set_item("symbol", "BTCUSDT").unwrap();
            dict.set_item("order_side", "Sell").unwrap();
            dict.set_item("order_type", "StopLimit").unwrap();
            dict.set_item("quantity", "1.0").unwrap();
            dict.set_item("price", "44500.00").unwrap();
            dict.set_item("stop_price", "45000.00").unwrap();

            let order = pydict_to_order(&dict).unwrap();
            assert_eq!(order.price, Some(dec!(44500.00)));
            assert_eq!(order.trigger_price, Some(dec!(45000.00)));
        });
    }

    #[test]
    fn test_pydict_to_order_time_in_force_variants() {
        Python::with_gil(|py| {
            // Note: GTD requires expire_time so we test it separately
            for (tif_str, expected) in [
                ("GTC", TimeInForce::GTC),
                ("IOC", TimeInForce::IOC),
                ("FOK", TimeInForce::FOK),
                ("DAY", TimeInForce::Day),
            ] {
                let dict = PyDict::new_bound(py);
                dict.set_item("symbol", "BTCUSDT").unwrap();
                dict.set_item("order_side", "Buy").unwrap();
                dict.set_item("order_type", "Market").unwrap();
                dict.set_item("quantity", "1.0").unwrap();
                dict.set_item("time_in_force", tif_str).unwrap();

                let order = pydict_to_order(&dict).unwrap();
                assert_eq!(order.time_in_force, expected, "Failed for TIF: {}", tif_str);
            }
        });
    }

    #[test]
    fn test_pydict_to_order_unknown_time_in_force_defaults_to_gtc() {
        Python::with_gil(|py| {
            let dict = PyDict::new_bound(py);
            dict.set_item("symbol", "BTCUSDT").unwrap();
            dict.set_item("order_side", "Buy").unwrap();
            dict.set_item("order_type", "Market").unwrap();
            dict.set_item("quantity", "1.0").unwrap();
            dict.set_item("time_in_force", "UNKNOWN").unwrap();

            let order = pydict_to_order(&dict).unwrap();
            assert_eq!(order.time_in_force, TimeInForce::GTC);
        });
    }

    #[test]
    fn test_pydict_to_order_with_client_order_id() {
        Python::with_gil(|py| {
            let dict = PyDict::new_bound(py);
            dict.set_item("symbol", "BTCUSDT").unwrap();
            dict.set_item("order_side", "Buy").unwrap();
            dict.set_item("order_type", "Market").unwrap();
            dict.set_item("quantity", "1.0").unwrap();
            dict.set_item("client_order_id", "my-custom-id-123")
                .unwrap();

            let order = pydict_to_order(&dict).unwrap();
            assert_eq!(order.client_order_id.as_str(), "my-custom-id-123");
        });
    }

    #[test]
    fn test_pydict_to_order_invalid_side() {
        Python::with_gil(|py| {
            let dict = PyDict::new_bound(py);
            dict.set_item("symbol", "BTCUSDT").unwrap();
            dict.set_item("order_side", "Invalid").unwrap();
            dict.set_item("order_type", "Market").unwrap();
            dict.set_item("quantity", "1.0").unwrap();

            let result = pydict_to_order(&dict);
            assert!(result.is_err());
        });
    }

    #[test]
    fn test_pydict_to_order_unsupported_type() {
        Python::with_gil(|py| {
            let dict = PyDict::new_bound(py);
            dict.set_item("symbol", "BTCUSDT").unwrap();
            dict.set_item("order_side", "Buy").unwrap();
            dict.set_item("order_type", "TrailingStop").unwrap();
            dict.set_item("quantity", "1.0").unwrap();

            let result = pydict_to_order(&dict);
            assert!(result.is_err());
        });
    }

    #[test]
    fn test_pydict_to_order_missing_required_fields() {
        Python::with_gil(|py| {
            // Missing symbol
            let dict = PyDict::new_bound(py);
            dict.set_item("order_side", "Buy").unwrap();
            dict.set_item("order_type", "Market").unwrap();
            dict.set_item("quantity", "1.0").unwrap();
            assert!(pydict_to_order(&dict).is_err());

            // Missing order_side
            let dict = PyDict::new_bound(py);
            dict.set_item("symbol", "BTCUSDT").unwrap();
            dict.set_item("order_type", "Market").unwrap();
            dict.set_item("quantity", "1.0").unwrap();
            assert!(pydict_to_order(&dict).is_err());

            // Missing order_type
            let dict = PyDict::new_bound(py);
            dict.set_item("symbol", "BTCUSDT").unwrap();
            dict.set_item("order_side", "Buy").unwrap();
            dict.set_item("quantity", "1.0").unwrap();
            assert!(pydict_to_order(&dict).is_err());

            // Missing quantity
            let dict = PyDict::new_bound(py);
            dict.set_item("symbol", "BTCUSDT").unwrap();
            dict.set_item("order_side", "Buy").unwrap();
            dict.set_item("order_type", "Market").unwrap();
            assert!(pydict_to_order(&dict).is_err());
        });
    }

    // ========================================================================
    // bar_data_to_pydict tests
    // ========================================================================

    fn create_test_bar_data() -> BarData {
        let ohlc = OHLCData {
            timestamp: Utc::now(),
            symbol: "BTCUSDT".to_string(),
            timeframe: Timeframe::OneMinute,
            open: dec!(50000.0),
            high: dec!(50100.0),
            low: dec!(49900.0),
            close: dec!(50050.0),
            volume: dec!(100.5),
            trade_count: 150,
        };

        let metadata = BarMetadata {
            bar_type: BarType::TimeBased(Timeframe::OneMinute),
            is_first_tick_of_bar: true,
            is_bar_closed: false,
            tick_count_in_bar: 10,
            is_synthetic: false,
            generation_timestamp: Utc::now(),
            is_session_truncated: false,
            is_session_aligned: false,
            is_historical: true,
        };

        BarData {
            current_tick: None,
            ohlc_bar: ohlc,
            metadata,
        }
    }

    fn create_test_bar_data_with_tick() -> BarData {
        let mut bar_data = create_test_bar_data();
        bar_data.current_tick = Some(TickData {
            symbol: "BTCUSDT".to_string(),
            timestamp: Utc::now(),
            ts_recv: Utc::now(),
            exchange: "BINANCE".to_string(),
            price: dec!(50050.0),
            quantity: dec!(0.5),
            side: TradeSide::Buy,
            provider: "BINANCE".to_string(),
            trade_id: "trade-123".to_string(),
            is_buyer_maker: false,
            sequence: 1,
            raw_dbn: None,
        });
        bar_data
    }

    #[test]
    fn test_bar_data_to_pydict_basic() {
        Python::with_gil(|py| {
            let bar_data = create_test_bar_data();
            let dict = bar_data_to_pydict(py, &bar_data).unwrap();

            // Check OHLC fields
            assert_eq!(
                dict.get_item("symbol")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "BTCUSDT"
            );
            assert_eq!(
                dict.get_item("open")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "50000.0"
            );
            assert_eq!(
                dict.get_item("high")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "50100.0"
            );
            assert_eq!(
                dict.get_item("low")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "49900.0"
            );
            assert_eq!(
                dict.get_item("close")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "50050.0"
            );
            assert_eq!(
                dict.get_item("volume")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "100.5"
            );
            assert_eq!(
                dict.get_item("trade_count")
                    .unwrap()
                    .unwrap()
                    .extract::<u64>()
                    .unwrap(),
                150
            );

            // Check metadata fields
            assert_eq!(
                dict.get_item("is_first_tick_of_bar")
                    .unwrap()
                    .unwrap()
                    .extract::<bool>()
                    .unwrap(),
                true
            );
            assert_eq!(
                dict.get_item("is_bar_closed")
                    .unwrap()
                    .unwrap()
                    .extract::<bool>()
                    .unwrap(),
                false
            );
            assert_eq!(
                dict.get_item("is_synthetic")
                    .unwrap()
                    .unwrap()
                    .extract::<bool>()
                    .unwrap(),
                false
            );
            assert_eq!(
                dict.get_item("tick_count_in_bar")
                    .unwrap()
                    .unwrap()
                    .extract::<u64>()
                    .unwrap(),
                10
            );

            // Check current_tick is None
            assert!(dict.get_item("current_tick").unwrap().unwrap().is_none());
        });
    }

    #[test]
    fn test_bar_data_to_pydict_with_tick() {
        Python::with_gil(|py| {
            let bar_data = create_test_bar_data_with_tick();
            let dict = bar_data_to_pydict(py, &bar_data).unwrap();

            // Check current_tick is present
            let tick_item = dict.get_item("current_tick").unwrap().unwrap();
            assert!(!tick_item.is_none());

            let tick_dict = tick_item.downcast::<PyDict>().unwrap();
            assert_eq!(
                tick_dict
                    .get_item("price")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "50050.0"
            );
            assert_eq!(
                tick_dict
                    .get_item("quantity")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "0.5"
            );
            assert_eq!(
                tick_dict
                    .get_item("side")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "Buy"
            );
            assert_eq!(
                tick_dict
                    .get_item("trade_id")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "trade-123"
            );
        });
    }

    #[test]
    fn test_bar_data_to_pydict_sell_side_tick() {
        Python::with_gil(|py| {
            let mut bar_data = create_test_bar_data_with_tick();
            if let Some(ref mut tick) = bar_data.current_tick {
                tick.side = TradeSide::Sell;
            }

            let dict = bar_data_to_pydict(py, &bar_data).unwrap();
            let tick_item = dict.get_item("current_tick").unwrap().unwrap();
            let tick_dict = tick_item.downcast::<PyDict>().unwrap();
            assert_eq!(
                tick_dict
                    .get_item("side")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "Sell"
            );
        });
    }

    // ========================================================================
    // PythonStrategy integration tests
    // ========================================================================

    #[test]
    fn test_python_strategy_load_sma_example() {
        // This test requires the example_sma.py file to exist
        let strategy_path = "strategies/examples/example_sma.py";
        if !std::path::Path::new(strategy_path).exists() {
            eprintln!("Skipping test: {} not found", strategy_path);
            return;
        }

        let result = PythonStrategy::from_file(strategy_path, "SMAStrategy");
        match result {
            Ok(strategy) => {
                assert_eq!(strategy.name(), "SMA Crossover Strategy");
            }
            Err(e) => {
                // May fail if RestrictedPython not installed
                if e.contains("RestrictedPython") {
                    eprintln!("Skipping test: RestrictedPython not installed");
                    return;
                }
                panic!("Failed to load strategy: {}", e);
            }
        }
    }

    #[test]
    fn test_python_strategy_load_nonexistent_file() {
        let result = PythonStrategy::from_file("nonexistent/path/strategy.py", "Strategy");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to read strategy file"));
    }

    #[test]
    fn test_python_strategy_load_nonexistent_class() {
        let strategy_path = "strategies/examples/example_sma.py";
        if !std::path::Path::new(strategy_path).exists() {
            eprintln!("Skipping test: {} not found", strategy_path);
            return;
        }

        let result = PythonStrategy::from_file(strategy_path, "NonExistentClass");
        match result {
            Err(e) => {
                // Should fail with class not found or RestrictedPython not installed
                if !e.contains("RestrictedPython") {
                    assert!(e.contains("not found") || e.contains("NonExistentClass"));
                }
            }
            Ok(_) => panic!("Should have failed to load non-existent class"),
        }
    }

    #[test]
    fn test_python_strategy_metrics_tracking() {
        let strategy_path = "strategies/examples/example_sma.py";
        if !std::path::Path::new(strategy_path).exists() {
            eprintln!("Skipping test: {} not found", strategy_path);
            return;
        }

        let result = PythonStrategy::from_file(strategy_path, "SMAStrategy");
        match result {
            Ok(strategy) => {
                // Initial metrics should be zero
                assert_eq!(strategy.get_cpu_time_us(), 0);
                assert_eq!(strategy.get_call_count(), 0);
                assert_eq!(strategy.get_peak_execution_us(), 0);
                assert_eq!(strategy.get_avg_execution_us(), 0);
            }
            Err(e) => {
                if e.contains("RestrictedPython") {
                    eprintln!("Skipping test: RestrictedPython not installed");
                    return;
                }
                panic!("Failed to load strategy: {}", e);
            }
        }
    }

    #[test]
    fn test_python_strategy_bar_data_mode_default() {
        let strategy_path = "strategies/examples/example_sma.py";
        if !std::path::Path::new(strategy_path).exists() {
            eprintln!("Skipping test: {} not found", strategy_path);
            return;
        }

        let result = PythonStrategy::from_file(strategy_path, "SMAStrategy");
        match result {
            Ok(strategy) => {
                // SMA strategy uses OnCloseBar
                let mode = strategy.bar_data_mode();
                assert_eq!(mode, BarDataMode::OnCloseBar);
            }
            Err(e) => {
                if e.contains("RestrictedPython") {
                    eprintln!("Skipping test: RestrictedPython not installed");
                    return;
                }
                panic!("Failed to load strategy: {}", e);
            }
        }
    }

    #[test]
    fn test_python_strategy_warmup_period() {
        let strategy_path = "strategies/examples/example_sma.py";
        if !std::path::Path::new(strategy_path).exists() {
            eprintln!("Skipping test: {} not found", strategy_path);
            return;
        }

        let result = PythonStrategy::from_file(strategy_path, "SMAStrategy");
        match result {
            Ok(strategy) => {
                // SMA strategy should have warmup period of long_period (default 20)
                let warmup = strategy.warmup_period();
                assert!(warmup > 0, "Warmup period should be > 0, got {}", warmup);
            }
            Err(e) => {
                if e.contains("RestrictedPython") {
                    eprintln!("Skipping test: RestrictedPython not installed");
                    return;
                }
                panic!("Failed to load strategy: {}", e);
            }
        }
    }

    #[test]
    fn test_python_strategy_initialize_with_params() {
        let strategy_path = "strategies/examples/example_sma.py";
        if !std::path::Path::new(strategy_path).exists() {
            eprintln!("Skipping test: {} not found", strategy_path);
            return;
        }

        let result = PythonStrategy::from_file(strategy_path, "SMAStrategy");
        match result {
            Ok(mut strategy) => {
                let mut params = HashMap::new();
                params.insert("short_period".to_string(), "10".to_string());
                params.insert("long_period".to_string(), "30".to_string());

                let init_result = strategy.initialize(params);
                assert!(
                    init_result.is_ok(),
                    "Initialize should succeed: {:?}",
                    init_result
                );
            }
            Err(e) => {
                if e.contains("RestrictedPython") {
                    eprintln!("Skipping test: RestrictedPython not installed");
                    return;
                }
                panic!("Failed to load strategy: {}", e);
            }
        }
    }

    #[test]
    fn test_python_strategy_on_bar_data_execution() {
        let strategy_path = "strategies/examples/example_sma.py";
        if !std::path::Path::new(strategy_path).exists() {
            eprintln!("Skipping test: {} not found", strategy_path);
            return;
        }

        let result = PythonStrategy::from_file(strategy_path, "SMAStrategy");
        match result {
            Ok(mut strategy) => {
                let bar_data = create_test_bar_data();
                let mut bars_context = BarsContext::new("BTCUSDT");

                // Execute on_bar_data (returns () now, orders via get_orders())
                strategy.on_bar_data(&bar_data, &mut bars_context);

                // Get orders (should be empty with not enough data for SMA)
                let orders = strategy.get_orders(&bar_data, &mut bars_context);
                assert!(
                    orders.is_empty(),
                    "Expected no orders with insufficient SMA data"
                );

                // Metrics should be updated
                assert!(strategy.get_call_count() >= 1);
                assert!(strategy.get_cpu_time_us() > 0);
            }
            Err(e) => {
                if e.contains("RestrictedPython") {
                    eprintln!("Skipping test: RestrictedPython not installed");
                    return;
                }
                panic!("Failed to load strategy: {}", e);
            }
        }
    }

    #[test]
    fn test_python_strategy_reset() {
        let strategy_path = "strategies/examples/example_sma.py";
        if !std::path::Path::new(strategy_path).exists() {
            eprintln!("Skipping test: {} not found", strategy_path);
            return;
        }

        let result = PythonStrategy::from_file(strategy_path, "SMAStrategy");
        match result {
            Ok(mut strategy) => {
                // Reset should not panic
                strategy.reset();
            }
            Err(e) => {
                if e.contains("RestrictedPython") {
                    eprintln!("Skipping test: RestrictedPython not installed");
                    return;
                }
                panic!("Failed to load strategy: {}", e);
            }
        }
    }

    #[test]
    fn test_python_strategy_preferred_bar_type() {
        let strategy_path = "strategies/examples/example_sma.py";
        if !std::path::Path::new(strategy_path).exists() {
            eprintln!("Skipping test: {} not found", strategy_path);
            return;
        }

        let result = PythonStrategy::from_file(strategy_path, "SMAStrategy");
        match result {
            Ok(strategy) => {
                let bar_type = strategy.preferred_bar_type();
                // SMA strategy uses 1-minute bars
                assert!(matches!(bar_type, BarType::TimeBased(Timeframe::OneMinute)));
            }
            Err(e) => {
                if e.contains("RestrictedPython") {
                    eprintln!("Skipping test: RestrictedPython not installed");
                    return;
                }
                panic!("Failed to load strategy: {}", e);
            }
        }
    }

    // ========================================================================
    // Error Recovery Tests
    // ========================================================================

    #[test]
    fn test_error_recovery_config_default() {
        let config = ErrorRecoveryConfig::default();
        assert_eq!(config.max_consecutive_errors, 10);
        assert!(config.reset_on_success);
        assert!(config.log_errors);
        assert!(config.circuit_breaker_enabled);
    }

    #[test]
    fn test_error_stats_default() {
        let stats = ErrorStats::default();
        assert_eq!(stats.total_errors, 0);
        assert_eq!(stats.consecutive_errors, 0);
        assert_eq!(stats.conversion_errors, 0);
        assert_eq!(stats.method_errors, 0);
        assert!(stats.last_error.is_none());
        assert!(stats.last_error_time.is_none());
    }

    #[test]
    fn test_python_bridge_error_display() {
        let err1 = PythonBridgeError::ConversionError("test".to_string());
        assert!(err1.to_string().contains("Conversion error"));

        let err2 = PythonBridgeError::MethodCallError("method failed".to_string());
        assert!(err2.to_string().contains("Method call error"));

        let err3 = PythonBridgeError::PythonException("exception".to_string());
        assert!(err3.to_string().contains("Python exception"));

        let err4 = PythonBridgeError::CircuitBreakerOpen;
        assert!(err4.to_string().contains("Circuit breaker"));

        let err5 = PythonBridgeError::GilTimeout;
        assert!(err5.to_string().contains("GIL"));
    }

    #[test]
    fn test_error_recovery_circuit_breaker() {
        let strategy_path = "strategies/examples/example_sma.py";
        if !std::path::Path::new(strategy_path).exists() {
            eprintln!("Skipping test: {} not found", strategy_path);
            return;
        }

        let result = PythonStrategy::from_file(strategy_path, "SMAStrategy");
        match result {
            Ok(mut strategy) => {
                // Configure for quick circuit breaker trigger
                strategy.set_error_config(ErrorRecoveryConfig {
                    max_consecutive_errors: 3,
                    reset_on_success: true,
                    log_errors: false, // Don't spam test output
                    circuit_breaker_enabled: true,
                });

                // Verify circuit is closed initially
                assert!(!strategy.is_circuit_open());
                assert_eq!(strategy.get_consecutive_errors(), 0);

                // Simulate errors
                strategy.simulate_error(
                    PythonBridgeError::ConversionError("test error 1".to_string()),
                    "test",
                );
                assert_eq!(strategy.get_consecutive_errors(), 1);
                assert!(!strategy.is_circuit_open());

                strategy.simulate_error(
                    PythonBridgeError::MethodCallError("test error 2".to_string()),
                    "test",
                );
                assert_eq!(strategy.get_consecutive_errors(), 2);
                assert!(!strategy.is_circuit_open());

                // Third error should open circuit
                strategy.simulate_error(
                    PythonBridgeError::PythonException("test error 3".to_string()),
                    "test",
                );
                assert_eq!(strategy.get_consecutive_errors(), 3);
                assert!(strategy.is_circuit_open());

                // Verify error stats
                let stats = strategy.get_error_stats();
                assert_eq!(stats.total_errors, 3);
                assert_eq!(stats.consecutive_errors, 3);
                assert!(stats.last_error.is_some());
                assert!(stats.last_error_time.is_some());
            }
            Err(e) => {
                if e.contains("RestrictedPython") {
                    eprintln!("Skipping test: RestrictedPython not installed");
                    return;
                }
                panic!("Failed to load strategy: {}", e);
            }
        }
    }

    #[test]
    fn test_error_recovery_reset_on_success() {
        let strategy_path = "strategies/examples/example_sma.py";
        if !std::path::Path::new(strategy_path).exists() {
            eprintln!("Skipping test: {} not found", strategy_path);
            return;
        }

        let result = PythonStrategy::from_file(strategy_path, "SMAStrategy");
        match result {
            Ok(mut strategy) => {
                strategy.set_error_config(ErrorRecoveryConfig {
                    max_consecutive_errors: 10,
                    reset_on_success: true,
                    log_errors: false,
                    circuit_breaker_enabled: true,
                });

                // Simulate some errors
                strategy.simulate_error(
                    PythonBridgeError::ConversionError("error 1".to_string()),
                    "test",
                );
                strategy.simulate_error(
                    PythonBridgeError::ConversionError("error 2".to_string()),
                    "test",
                );
                assert_eq!(strategy.get_consecutive_errors(), 2);

                // Execute successful bar data (will reset consecutive count)
                let bar_data = create_test_bar_data();
                let mut bars_context = BarsContext::new("BTCUSDT");
                let _signal = strategy.on_bar_data(&bar_data, &mut bars_context);

                // Consecutive errors should reset (on success)
                assert_eq!(strategy.get_consecutive_errors(), 0);

                // But total errors remain
                let stats = strategy.get_error_stats();
                assert_eq!(stats.total_errors, 2);
            }
            Err(e) => {
                if e.contains("RestrictedPython") {
                    eprintln!("Skipping test: RestrictedPython not installed");
                    return;
                }
                panic!("Failed to load strategy: {}", e);
            }
        }
    }

    #[test]
    fn test_error_recovery_circuit_breaker_reset() {
        let strategy_path = "strategies/examples/example_sma.py";
        if !std::path::Path::new(strategy_path).exists() {
            eprintln!("Skipping test: {} not found", strategy_path);
            return;
        }

        let result = PythonStrategy::from_file(strategy_path, "SMAStrategy");
        match result {
            Ok(mut strategy) => {
                strategy.set_error_config(ErrorRecoveryConfig {
                    max_consecutive_errors: 2,
                    reset_on_success: true,
                    log_errors: false,
                    circuit_breaker_enabled: true,
                });

                // Trigger circuit breaker
                strategy.simulate_error(
                    PythonBridgeError::PythonException("err1".to_string()),
                    "test",
                );
                strategy.simulate_error(
                    PythonBridgeError::PythonException("err2".to_string()),
                    "test",
                );
                assert!(strategy.is_circuit_open());

                // Manually reset circuit breaker
                strategy.reset_circuit_breaker();
                assert!(!strategy.is_circuit_open());
                assert_eq!(strategy.get_consecutive_errors(), 0);
            }
            Err(e) => {
                if e.contains("RestrictedPython") {
                    eprintln!("Skipping test: RestrictedPython not installed");
                    return;
                }
                panic!("Failed to load strategy: {}", e);
            }
        }
    }

    #[test]
    fn test_error_recovery_callback() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let strategy_path = "strategies/examples/example_sma.py";
        if !std::path::Path::new(strategy_path).exists() {
            eprintln!("Skipping test: {} not found", strategy_path);
            return;
        }

        let result = PythonStrategy::from_file(strategy_path, "SMAStrategy");
        match result {
            Ok(mut strategy) => {
                strategy.set_error_config(ErrorRecoveryConfig {
                    max_consecutive_errors: 10,
                    reset_on_success: true,
                    log_errors: false,
                    circuit_breaker_enabled: false,
                });

                // Set up callback to count errors
                let error_count = Arc::new(AtomicUsize::new(0));
                let error_count_clone = error_count.clone();

                strategy.set_error_callback(Box::new(move |_error, _context| {
                    error_count_clone.fetch_add(1, Ordering::SeqCst);
                }));

                // Simulate errors
                strategy.simulate_error(
                    PythonBridgeError::ConversionError("test".to_string()),
                    "test",
                );
                strategy.simulate_error(
                    PythonBridgeError::MethodCallError("test".to_string()),
                    "test",
                );

                // Callback should have been called twice
                assert_eq!(error_count.load(Ordering::SeqCst), 2);
            }
            Err(e) => {
                if e.contains("RestrictedPython") {
                    eprintln!("Skipping test: RestrictedPython not installed");
                    return;
                }
                panic!("Failed to load strategy: {}", e);
            }
        }
    }

    #[test]
    fn test_error_recovery_circuit_breaker_disables_on_bar_data() {
        let strategy_path = "strategies/examples/example_sma.py";
        if !std::path::Path::new(strategy_path).exists() {
            eprintln!("Skipping test: {} not found", strategy_path);
            return;
        }

        let result = PythonStrategy::from_file(strategy_path, "SMAStrategy");
        match result {
            Ok(mut strategy) => {
                strategy.set_error_config(ErrorRecoveryConfig {
                    max_consecutive_errors: 2,
                    reset_on_success: true,
                    log_errors: false,
                    circuit_breaker_enabled: true,
                });

                // Trigger circuit breaker
                strategy.simulate_error(
                    PythonBridgeError::PythonException("err1".to_string()),
                    "test",
                );
                strategy.simulate_error(
                    PythonBridgeError::PythonException("err2".to_string()),
                    "test",
                );
                assert!(strategy.is_circuit_open());

                // on_bar_data should do nothing with circuit open (no Python call)
                let bar_data = create_test_bar_data();
                let mut bars_context = BarsContext::new("BTCUSDT");
                strategy.on_bar_data(&bar_data, &mut bars_context);

                // Get orders should return empty when circuit is open
                let orders = strategy.get_orders(&bar_data, &mut bars_context);
                assert!(orders.is_empty(), "Circuit open should return no orders");

                // Call count should still be 0 (circuit breaker prevented call)
                assert_eq!(strategy.get_call_count(), 0);
            }
            Err(e) => {
                if e.contains("RestrictedPython") {
                    eprintln!("Skipping test: RestrictedPython not installed");
                    return;
                }
                panic!("Failed to load strategy: {}", e);
            }
        }
    }

    #[test]
    fn test_error_stats_reset() {
        let strategy_path = "strategies/examples/example_sma.py";
        if !std::path::Path::new(strategy_path).exists() {
            eprintln!("Skipping test: {} not found", strategy_path);
            return;
        }

        let result = PythonStrategy::from_file(strategy_path, "SMAStrategy");
        match result {
            Ok(mut strategy) => {
                strategy.set_error_config(ErrorRecoveryConfig {
                    max_consecutive_errors: 10,
                    reset_on_success: false, // Don't reset on success
                    log_errors: false,
                    circuit_breaker_enabled: false,
                });

                // Accumulate some errors
                strategy.simulate_error(
                    PythonBridgeError::ConversionError("err".to_string()),
                    "test",
                );
                strategy.simulate_error(
                    PythonBridgeError::ConversionError("err".to_string()),
                    "test",
                );
                strategy.simulate_error(
                    PythonBridgeError::ConversionError("err".to_string()),
                    "test",
                );

                let stats = strategy.get_error_stats();
                assert_eq!(stats.total_errors, 3);

                // Reset error stats
                strategy.reset_error_stats();

                let stats = strategy.get_error_stats();
                assert_eq!(stats.total_errors, 0);
                assert_eq!(stats.consecutive_errors, 0);
                assert!(!strategy.is_circuit_open());
            }
            Err(e) => {
                if e.contains("RestrictedPython") {
                    eprintln!("Skipping test: RestrictedPython not installed");
                    return;
                }
                panic!("Failed to load strategy: {}", e);
            }
        }
    }
}
