use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::data::types::{TickData, OHLCData, Timeframe, TradeSide};
use super::base::{Strategy, Signal};
use rust_decimal::Decimal;
use std::str::FromStr;

/// Wrapper around Python strategy instance
pub struct PythonStrategy {
    /// Python object instance (stored as PyObject to avoid GIL issues)
    py_instance: Arc<Mutex<PyObject>>,
    /// Cached strategy name
    cached_name: String,
    /// Whether strategy supports OHLC
    supports_ohlc_cached: bool,
    /// Preferred timeframe (cached)
    preferred_timeframe_cached: Option<Timeframe>,
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
            if let Some(parent) = std::path::Path::new(path).parent() {
                py_path.call_method1("insert", (0, parent.to_str().unwrap()))
                    .map_err(|e| format!("Failed to add to sys.path: {}", e))?;
            }

            // Load the Python module from file
            let code = std::fs::read_to_string(path)
                .map_err(|e| format!("Failed to read strategy file: {}", e))?;

            let module_name = std::path::Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or("Invalid file name")?;

            let module = PyModule::from_code_bound(py, &code, path, module_name)
                .map_err(|e| format!("Failed to load Python module: {}", e))?;

            // Get the strategy class
            let strategy_class = module.getattr(class_name)
                .map_err(|e| format!("Failed to find class '{}': {}", class_name, e))?;

            // Instantiate the strategy
            let instance = strategy_class.call0()
                .map_err(|e| format!("Failed to instantiate strategy: {}", e))?;

            // Cache metadata
            let name = instance.call_method0("name")
                .and_then(|n| n.extract::<String>())
                .map_err(|e| format!("Failed to get strategy name: {}", e))?;

            let supports_ohlc = instance.call_method0("supports_ohlc")
                .and_then(|b| b.extract::<bool>())
                .unwrap_or(false);

            let preferred_timeframe = instance.call_method0("preferred_timeframe")
                .and_then(|t: Bound<'_, PyAny>| t.extract::<Option<String>>())
                .ok()
                .flatten()
                .and_then(|s| parse_timeframe(&s));

            Ok(Self {
                py_instance: Arc::new(Mutex::new(instance.into())),
                cached_name: name,
                supports_ohlc_cached: supports_ohlc,
                preferred_timeframe_cached: preferred_timeframe,
            })
        })
    }
}

/// Convert Rust TickData to Python dict
fn tick_to_pydict<'a>(py: Python<'a>, tick: &TickData) -> PyResult<Bound<'a, PyDict>> {
    let dict = PyDict::new_bound(py);
    dict.set_item("timestamp", tick.timestamp.to_rfc3339())?;
    dict.set_item("symbol", &tick.symbol)?;
    dict.set_item("price", tick.price.to_string())?;
    dict.set_item("quantity", tick.quantity.to_string())?;
    dict.set_item("side", match tick.side {
        TradeSide::Buy => "Buy",
        TradeSide::Sell => "Sell",
    })?;
    dict.set_item("trade_id", &tick.trade_id)?;
    dict.set_item("is_buyer_maker", tick.is_buyer_maker)?;
    Ok(dict)
}

/// Convert Rust OHLCData to Python dict
fn ohlc_to_pydict<'a>(py: Python<'a>, ohlc: &OHLCData) -> PyResult<Bound<'a, PyDict>> {
    let dict = PyDict::new_bound(py);
    dict.set_item("timestamp", ohlc.timestamp.to_rfc3339())?;
    dict.set_item("symbol", &ohlc.symbol)?;
    dict.set_item("timeframe", timeframe_to_string(&ohlc.timeframe))?;
    dict.set_item("open", ohlc.open.to_string())?;
    dict.set_item("high", ohlc.high.to_string())?;
    dict.set_item("low", ohlc.low.to_string())?;
    dict.set_item("close", ohlc.close.to_string())?;
    dict.set_item("volume", ohlc.volume.to_string())?;
    dict.set_item("trade_count", ohlc.trade_count)?;
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

/// Convert Timeframe enum to string
fn timeframe_to_string(tf: &Timeframe) -> &'static str {
    match tf {
        Timeframe::OneMinute => "1m",
        Timeframe::FiveMinutes => "5m",
        Timeframe::FifteenMinutes => "15m",
        Timeframe::ThirtyMinutes => "30m",
        Timeframe::OneHour => "1h",
        Timeframe::FourHours => "4h",
        Timeframe::OneDay => "1d",
        Timeframe::OneWeek => "1w",
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

    fn on_tick(&mut self, tick: &TickData) -> Signal {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            // Convert tick to Python dict
            let tick_dict = match tick_to_pydict(py, tick) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("Failed to convert tick to Python: {}", e);
                    return Signal::Hold;
                }
            };

            // Call Python method
            match py_instance.call_method1("on_tick", (tick_dict,)) {
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
                    eprintln!("Python on_tick error: {}", e);
                    Signal::Hold
                }
            }
        })
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
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            if let Err(e) = py_instance.call_method0("reset") {
                eprintln!("Python reset error: {}", e);
            }
        });
    }

    fn on_ohlc(&mut self, ohlc: &OHLCData) -> Signal {
        Python::with_gil(|py| {
            let instance = self.py_instance.lock().unwrap();
            let py_instance = instance.bind(py);

            let ohlc_dict = match ohlc_to_pydict(py, ohlc) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("Failed to convert OHLC to Python: {}", e);
                    return Signal::Hold;
                }
            };

            match py_instance.call_method1("on_ohlc", (ohlc_dict,)) {
                Ok(result) => {
                    match pydict_to_signal(&result) {
                        Ok(signal) => signal,
                        Err(e) => {
                            eprintln!("Failed to convert Python signal: {}", e);
                            Signal::Hold
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Python on_ohlc error: {}", e);
                    Signal::Hold
                }
            }
        })
    }

    fn supports_ohlc(&self) -> bool {
        self.supports_ohlc_cached
    }

    fn preferred_timeframe(&self) -> Option<Timeframe> {
        self.preferred_timeframe_cached
    }
}

// Thread safety: PythonStrategy is Send + Sync because:
// 1. PyObject is wrapped in Arc<Mutex<>>
// 2. All Python access is guarded by GIL via Python::with_gil()
// 3. Cached fields are immutable after construction
unsafe impl Send for PythonStrategy {}
unsafe impl Sync for PythonStrategy {}
