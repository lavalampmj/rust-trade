use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use crate::data::types::{BarData, BarDataMode, BarType, Timeframe, TradeSide};
use crate::series::bars_context::BarsContext;
use crate::series::MaximumBarsLookBack;
// OHLCData included via BarData.ohlc_bar
use super::base::{Strategy, Signal};
use rust_decimal::Decimal;
use std::str::FromStr;

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
}

impl std::fmt::Debug for PythonStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PythonStrategy")
            .field("cached_name", &self.cached_name)
            .field("cpu_time_us", &self.cpu_time_us.load(Ordering::Relaxed))
            .field("call_count", &self.call_count.load(Ordering::Relaxed))
            .field("peak_execution_us", &self.peak_execution_us.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl PythonStrategy {
    /// Load strategy from Python file
    pub fn from_file(path: &str, class_name: &str) -> Result<Self, String> {
        Python::with_gil(|py| {
            // Add strategy directory to Python path
            let sys = py.import_bound("sys")
                .map_err(|e| format!("Failed to import sys: {}", e))?;
            let py_path = sys.getattr("path")
                .map_err(|e| format!("Failed to get sys.path: {}", e))?;

            // Add parent directory of strategy file to path
            let strategy_path = std::path::Path::new(path);
            if let Some(parent) = strategy_path.parent() {
                py_path.call_method1("insert", (0, parent.to_str().unwrap()))
                    .map_err(|e| format!("Failed to add to sys.path: {}", e))?;

                // Also add strategies root (for _lib package imports)
                // If parent is "examples" or "_tests", go up one more level
                if let Some(parent_name) = parent.file_name().and_then(|n| n.to_str()) {
                    if parent_name == "examples" || parent_name == "_tests" {
                        if let Some(strategies_root) = parent.parent() {
                            py_path.call_method1("insert", (0, strategies_root.to_str().unwrap()))
                                .map_err(|e| format!("Failed to add strategies root to sys.path: {}", e))?;

                            // Add _lib directory for restricted_compiler import
                            let lib_path = strategies_root.join("_lib");
                            if lib_path.exists() {
                                py_path.call_method1("insert", (0, lib_path.to_str().unwrap()))
                                    .map_err(|e| format!("Failed to add _lib to sys.path: {}", e))?;
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
            let restricted_compiler = py.import_bound("restricted_compiler")
                .map_err(|e| format!(
                    "Failed to import restricted_compiler: {}\n\
                     Make sure RestrictedPython is installed: pip install RestrictedPython==7.0\n\
                     And strategies/_lib/restricted_compiler.py exists",
                    e
                ))?;

            // Compile strategy code with restrictions
            let result = restricted_compiler
                .getattr("compile_strategy")
                .map_err(|e| format!("Failed to get compile_strategy function: {}", e))?
                .call1((code.as_str(), path))
                .map_err(|e| format!(
                    "Failed to compile strategy with restrictions: {}\n\
                     This usually means the strategy code violates security policies.\n\
                     Check for: prohibited imports (os, subprocess, urllib, socket, sys), \n\
                     eval/exec usage, or unsafe operations.",
                    e
                ))?;

            // Extract bytecode and globals from tuple
            let tuple = result.downcast::<PyTuple>()
                .map_err(|e| format!("compile_strategy should return tuple: {}", e))?;
            let bytecode = tuple.get_item(0)
                .map_err(|e| format!("Failed to get bytecode from tuple: {}", e))?;
            let globals_item = tuple.get_item(1)
                .map_err(|e| format!("Failed to get globals from tuple: {}", e))?;
            let restricted_globals = globals_item.downcast::<PyDict>()
                .map_err(|e| format!("restricted_globals should be dict: {}", e))?;

            // Set module metadata in restricted globals
            restricted_globals.set_item("__name__", module_name)
                .map_err(|e| format!("Failed to set __name__: {}", e))?;
            restricted_globals.set_item("__file__", path)
                .map_err(|e| format!("Failed to set __file__: {}", e))?;

            // Execute bytecode in restricted environment using Python's exec
            let builtins = py.import_bound("builtins")
                .map_err(|e| format!("Failed to import builtins: {}", e))?;
            let exec_fn = builtins.getattr("exec")
                .map_err(|e| format!("Failed to get exec function: {}", e))?;

            exec_fn.call1((bytecode, restricted_globals))
                .map_err(|e| format!(
                    "Failed to execute strategy code: {}\n\
                     The strategy may be trying to use prohibited operations.",
                    e
                ))?;

            // Get the strategy class from restricted globals
            let strategy_class = restricted_globals.get_item(class_name)
                .map_err(|e| format!("Failed to get item from globals: {}", e))?
                .ok_or_else(|| format!("Class '{}' not found in module", class_name))?;

            // Instantiate the strategy
            let instance = strategy_class.call0()
                .map_err(|e| format!("Failed to instantiate strategy: {}", e))?;

            // Cache metadata
            let name = instance.call_method0("name")
                .and_then(|n: Bound<'_, PyAny>| n.extract::<String>())
                .map_err(|e| format!("Failed to get strategy name: {}", e))?;

            // Get max_bars_lookback from strategy (default 256)
            let max_lookback = match instance.call_method0("max_bars_lookback") {
                Ok(result) => result.extract::<usize>().unwrap_or(256),
                Err(_) => 256,
            };

            // Import and create Python BarsContext
            let bars_module = py.import_bound("_lib.bars_context")
                .map_err(|e| format!("Failed to import _lib.bars_context: {}", e))?;
            let bars_class = bars_module.getattr("BarsContext")
                .map_err(|e| format!("Failed to get BarsContext class: {}", e))?;

            let py_bars = bars_class.call1(("", max_lookback))
                .map_err(|e| format!("Failed to create BarsContext: {}", e))?;

            Ok(Self {
                py_instance: Arc::new(Mutex::new(instance.into())),
                py_bars_context: Arc::new(Mutex::new(py_bars.into())),
                cached_name: name,
                cpu_time_us: Arc::new(AtomicU64::new(0)),
                call_count: Arc::new(AtomicU64::new(0)),
                peak_execution_us: Arc::new(AtomicU64::new(0)),
            })
        })
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
    dict.set_item("is_first_tick_of_bar", bar_data.metadata.is_first_tick_of_bar)?;
    dict.set_item("is_bar_closed", bar_data.metadata.is_bar_closed)?;
    dict.set_item("is_synthetic", bar_data.metadata.is_synthetic)?;
    dict.set_item("tick_count_in_bar", bar_data.metadata.tick_count_in_bar)?;

    // Add current tick if present
    if let Some(ref tick) = bar_data.current_tick {
        let tick_dict = PyDict::new_bound(py);
        tick_dict.set_item("timestamp", tick.timestamp.to_rfc3339())?;
        tick_dict.set_item("price", tick.price.to_string())?;
        tick_dict.set_item("quantity", tick.quantity.to_string())?;
        tick_dict.set_item("side", match tick.side {
            TradeSide::Buy => "Buy",
            TradeSide::Sell => "Sell",
        })?;
        tick_dict.set_item("trade_id", &tick.trade_id)?;
        dict.set_item("current_tick", tick_dict)?;
    } else {
        dict.set_item("current_tick", py.None())?;
    }

    Ok(dict)
}


/// Convert Python dict to Rust Signal
fn pydict_to_signal(obj: &Bound<'_, PyAny>) -> PyResult<Signal> {
    let dict = obj.downcast::<PyDict>()?;
    let signal_type = dict.get_item("type")?
        .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err("Missing 'type' field"))?
        .extract::<String>()?;

    match signal_type.as_str() {
        "Buy" => {
            let symbol = dict.get_item("symbol")?
                .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err("Missing 'symbol'"))?
                .extract::<String>()?;
            let quantity_str = dict.get_item("quantity")?
                .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err("Missing 'quantity'"))?
                .extract::<String>()?;
            let quantity = Decimal::from_str(&quantity_str)
                .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid quantity: {}", e)))?;
            Ok(Signal::Buy { symbol, quantity })
        }
        "Sell" => {
            let symbol = dict.get_item("symbol")?
                .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err("Missing 'symbol'"))?
                .extract::<String>()?;
            let quantity_str = dict.get_item("quantity")?
                .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err("Missing 'quantity'"))?
                .extract::<String>()?;
            let quantity = Decimal::from_str(&quantity_str)
                .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid quantity: {}", e)))?;
            Ok(Signal::Sell { symbol, quantity })
        }
        "Hold" => Ok(Signal::Hold),
        _ => Err(pyo3::exceptions::PyValueError::new_err(format!("Unknown signal type: {}", signal_type))),
    }
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

impl Strategy for PythonStrategy {
    fn name(&self) -> &str {
        &self.cached_name
    }

    fn on_bar_data(&mut self, bar_data: &BarData, _bars: &mut BarsContext) -> Signal {
        // Note: Python strategies use their own Python BarsContext
        // The Rust _bars parameter is ignored; we pass Python BarsContext instead

        // Start timing
        let start = std::time::Instant::now();

        let signal = Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_bars = self.py_bars_context.lock().unwrap();
            let py_instance = instance.bind(py);
            let py_bars_bound = py_bars.bind(py);

            // Convert BarData to Python dict
            let bar_dict = match bar_data_to_pydict(py, bar_data) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("Failed to convert BarData to Python: {}", e);
                    return Signal::Hold;
                }
            };

            // Update Python BarsContext BEFORE calling strategy
            if let Err(e) = py_bars_bound.call_method1("on_bar_update", (&bar_dict,)) {
                eprintln!("Failed to update Python BarsContext: {}", e);
                return Signal::Hold;
            }

            // Call Python on_bar_data method with BOTH bar_data AND bars
            match py_instance.call_method1("on_bar_data", (&bar_dict, py_bars_bound)) {
                Ok(result) => {
                    match pydict_to_signal(&result) {
                        Ok(signal) => signal,
                        Err(e) => {
                            eprintln!("Failed to convert Python signal to Rust: {}", e);
                            Signal::Hold
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Python on_bar_data error: {}", e);
                    Signal::Hold
                }
            }
        });

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

        signal
    }

    fn initialize(&mut self, params: HashMap<String, String>) -> Result<(), String> {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            // Convert HashMap to Python dict
            let py_params = PyDict::new_bound(py);
            for (k, v) in params.iter() {
                py_params.set_item(k, v)
                    .map_err(|e| format!("Failed to set param: {}", e))?;
            }

            // Call initialize
            let result = py_instance.call_method1("initialize", (&py_params,))
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
                            _ => BarDataMode::OnEachTick,
                        }
                    } else {
                        BarDataMode::OnEachTick
                    }
                }
                Err(_) => BarDataMode::OnEachTick,
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
                                        if let Ok(Some(count)) = bar_type_dict.get_item("tick_count") {
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
}

// Thread safety: PythonStrategy is Send + Sync because:
// 1. PyObject is wrapped in Arc<Mutex<>>
// 2. All Python access is guarded by GIL via Python::with_gil()
// 3. Cached fields are immutable after construction
unsafe impl Send for PythonStrategy {}
unsafe impl Sync for PythonStrategy {}
