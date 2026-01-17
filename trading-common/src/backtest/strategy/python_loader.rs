use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use notify::{Watcher, RecursiveMode, Event, EventKind, recommended_watcher};
use parking_lot::RwLock as ParkingLotRwLock;
use sha2::{Sha256, Digest};
use super::python_bridge::PythonStrategy;
use super::base::Strategy;

#[derive(Debug, Clone)]
pub struct StrategyConfig {
    pub id: String,
    pub file_path: PathBuf,
    pub class_name: String,
    pub description: String,
    pub enabled: bool,
    pub sha256: Option<String>,
}

/// Verify strategy file hash matches expected SHA256
fn verify_strategy_hash(path: &Path, expected_hash: &str) -> Result<(), String> {
    // Read file contents
    let code = std::fs::read(path)
        .map_err(|e| format!("Failed to read strategy file: {}", e))?;

    // Calculate SHA256 hash
    let mut hasher = Sha256::new();
    hasher.update(&code);
    let hash = hasher.finalize();
    let hash_hex = hex::encode(hash);

    // Compare hashes
    if hash_hex != expected_hash {
        return Err(format!(
            "Strategy hash mismatch for {:?}\nExpected: {}\nGot:      {}\n\
             This indicates the strategy file has been modified.\n\
             Update the sha256 field in your config or regenerate with: cargo run --bin trading-core -- hash-strategy {:?}",
            path, expected_hash, hash_hex, path
        ));
    }

    Ok(())
}

/// Calculate SHA256 hash of a file (utility function for CLI)
pub fn calculate_file_hash(path: &Path) -> Result<String, String> {
    let code = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let mut hasher = Sha256::new();
    hasher.update(&code);
    let hash = hasher.finalize();

    Ok(hex::encode(hash))
}

pub struct PythonStrategyRegistry {
    /// Registered strategies (from config)
    configs: Arc<RwLock<HashMap<String, StrategyConfig>>>,

    /// Cached loaded strategies
    /// Using parking_lot::RwLock for better performance
    loaded_strategies: Arc<ParkingLotRwLock<HashMap<String, Arc<PythonStrategy>>>>,

    /// File watcher for hot-reload
    _watcher: Option<Box<dyn Watcher + Send + Sync>>,

    /// Strategy directory path
    strategy_dir: PathBuf,
}

impl PythonStrategyRegistry {
    pub fn new(strategy_dir: PathBuf) -> Result<Self, String> {
        Ok(Self {
            configs: Arc::new(RwLock::new(HashMap::new())),
            loaded_strategies: Arc::new(ParkingLotRwLock::new(HashMap::new())),
            _watcher: None,
            strategy_dir,
        })
    }

    /// Register strategy from config
    pub fn register(&mut self, config: StrategyConfig) -> Result<(), String> {
        let mut configs = self.configs.write()
            .map_err(|_| "Failed to acquire write lock")?;

        // Validate file exists
        if !config.file_path.exists() {
            return Err(format!("Strategy file not found: {:?}", config.file_path));
        }

        configs.insert(config.id.clone(), config);
        Ok(())
    }

    /// Load strategy (or get from cache)
    pub fn get_strategy(&self, strategy_id: &str) -> Result<Box<dyn Strategy>, String> {
        // Check cache first
        {
            let cache = self.loaded_strategies.read();
            if let Some(strategy) = cache.get(strategy_id) {
                // Return a wrapper that works with Box<dyn Strategy>
                return Ok(Box::new(PythonStrategyWrapper {
                    inner: Arc::clone(strategy),
                }));
            }
        }

        // Load from config
        let config = {
            let configs = self.configs.read()
                .map_err(|_| "Failed to acquire read lock")?;
            configs.get(strategy_id)
                .ok_or_else(|| format!("Strategy '{}' not registered", strategy_id))?
                .clone()
        };

        if !config.enabled {
            return Err(format!("Strategy '{}' is disabled", strategy_id));
        }

        // Verify hash if provided in config
        if let Some(expected_hash) = &config.sha256 {
            verify_strategy_hash(&config.file_path, expected_hash)?;
        }

        // Load Python strategy
        let strategy = PythonStrategy::from_file(
            config.file_path.to_str().unwrap(),
            &config.class_name
        )?;

        let arc_strategy = Arc::new(strategy);

        // Cache it
        {
            let mut cache = self.loaded_strategies.write();
            cache.insert(strategy_id.to_string(), Arc::clone(&arc_strategy));
        }

        Ok(Box::new(PythonStrategyWrapper {
            inner: arc_strategy,
        }))
    }

    /// Enable hot-reload (start file watcher)
    pub fn enable_hot_reload(&mut self) -> Result<(), String> {
        let (tx, rx) = std::sync::mpsc::channel();

        let mut watcher = recommended_watcher(tx)
            .map_err(|e| format!("Failed to create watcher: {}", e))?;

        watcher.watch(&self.strategy_dir, RecursiveMode::NonRecursive)
            .map_err(|e| format!("Failed to watch directory: {}", e))?;

        let loaded_strategies = Arc::clone(&self.loaded_strategies);
        let configs = Arc::clone(&self.configs);

        // Spawn background thread for watching
        std::thread::spawn(move || {
            loop {
                match rx.recv() {
                    Ok(Ok(event)) => {
                        if matches!(event.kind, EventKind::Modify(_)) {
                            Self::handle_file_change(
                                &event,
                                &loaded_strategies,
                                &configs
                            );
                        }
                    }
                    Ok(Err(e)) => eprintln!("Watch error: {}", e),
                    Err(e) => {
                        eprintln!("Channel error: {}", e);
                        break;
                    }
                }
            }
        });

        self._watcher = Some(Box::new(watcher));
        println!("✓ Hot-reload enabled for {:?}", self.strategy_dir);
        Ok(())
    }

    fn handle_file_change(
        event: &Event,
        cache: &Arc<ParkingLotRwLock<HashMap<String, Arc<PythonStrategy>>>>,
        configs: &Arc<RwLock<HashMap<String, StrategyConfig>>>,
    ) {
        for path in &event.paths {
            println!("📝 File changed: {:?}", path);

            // Find which strategy config matches this path
            let configs_read = match configs.read() {
                Ok(c) => c,
                Err(_) => return,
            };

            let matching_ids: Vec<String> = configs_read
                .iter()
                .filter(|(_, config)| config.file_path == *path)
                .map(|(id, _)| id.clone())
                .collect();

            drop(configs_read);

            // Invalidate cache for matching strategies
            let mut cache_write = cache.write();
            for id in matching_ids {
                cache_write.remove(&id);
                println!("🔄 Invalidated cache for strategy: {}", id);
            }
        }
    }

    /// List all registered strategies
    pub fn list_strategies(&self) -> Vec<StrategyConfig> {
        self.configs.read()
            .map(|configs| configs.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Reload specific strategy (invalidate cache)
    pub fn reload_strategy(&self, strategy_id: &str) {
        let mut cache = self.loaded_strategies.write();
        cache.remove(strategy_id);
        println!("🔄 Reloaded strategy: {}", strategy_id);
    }
}

/// Wrapper to make Arc<PythonStrategy> implement Strategy
/// This allows us to return Box<dyn Strategy> while keeping the Arc for caching
struct PythonStrategyWrapper {
    inner: Arc<PythonStrategy>,
}

impl Strategy for PythonStrategyWrapper {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn on_tick(&mut self, tick: &crate::data::types::TickData) -> super::base::Signal {
        // We need mutable access but only have Arc
        // PythonStrategy uses internal Mutex for state, so this is safe
        unsafe {
            let ptr = Arc::as_ptr(&self.inner) as *mut PythonStrategy;
            (*ptr).on_tick(tick)
        }
    }

    fn initialize(&mut self, params: std::collections::HashMap<String, String>) -> Result<(), String> {
        unsafe {
            let ptr = Arc::as_ptr(&self.inner) as *mut PythonStrategy;
            (*ptr).initialize(params)
        }
    }

    fn reset(&mut self) {
        unsafe {
            let ptr = Arc::as_ptr(&self.inner) as *mut PythonStrategy;
            (*ptr).reset()
        }
    }

    fn on_ohlc(&mut self, ohlc: &crate::data::types::OHLCData) -> super::base::Signal {
        unsafe {
            let ptr = Arc::as_ptr(&self.inner) as *mut PythonStrategy;
            (*ptr).on_ohlc(ohlc)
        }
    }

    fn supports_ohlc(&self) -> bool {
        self.inner.supports_ohlc()
    }

    fn preferred_timeframe(&self) -> Option<crate::data::types::Timeframe> {
        self.inner.preferred_timeframe()
    }
}
