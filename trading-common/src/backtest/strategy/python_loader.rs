use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use notify::{Watcher, RecursiveMode, EventKind, recommended_watcher};
use parking_lot::RwLock as ParkingLotRwLock;
use sha2::{Sha256, Digest};
use super::python_bridge::PythonStrategy;
use super::base::Strategy;
use crate::series::bars_context::BarsContext;
use crate::series::MaximumBarsLookBack;

#[derive(Debug, Clone)]
pub struct StrategyConfig {
    pub id: String,
    pub file_path: PathBuf,
    pub class_name: String,
    pub description: String,
    pub enabled: bool,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HotReloadConfig {
    /// Enable debouncing (wait before reloading after file change)
    pub debounce_ms: u64,
    /// Skip hash verification on hot-reload (development mode only)
    pub skip_hash_verification: bool,
}

impl Default for HotReloadConfig {
    fn default() -> Self {
        Self {
            debounce_ms: 300, // 300ms default debounce
            skip_hash_verification: false,
        }
    }
}

/// Metrics for hot-reload monitoring
#[derive(Debug, Clone)]
pub struct ReloadMetrics {
    pub total_reloads: Arc<AtomicU64>,
    pub successful_reloads: Arc<AtomicU64>,
    pub failed_reloads: Arc<AtomicU64>,
    pub last_reload_timestamp: Arc<AtomicU64>,
}

impl Default for ReloadMetrics {
    fn default() -> Self {
        Self {
            total_reloads: Arc::new(AtomicU64::new(0)),
            successful_reloads: Arc::new(AtomicU64::new(0)),
            failed_reloads: Arc::new(AtomicU64::new(0)),
            last_reload_timestamp: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl ReloadMetrics {
    pub fn record_success(&self) {
        self.total_reloads.fetch_add(1, Ordering::Relaxed);
        self.successful_reloads.fetch_add(1, Ordering::Relaxed);
        self.update_timestamp();
    }

    pub fn record_failure(&self) {
        self.total_reloads.fetch_add(1, Ordering::Relaxed);
        self.failed_reloads.fetch_add(1, Ordering::Relaxed);
        self.update_timestamp();
    }

    fn update_timestamp(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();
        self.last_reload_timestamp.store(now, Ordering::Relaxed);
    }

    pub fn get_stats(&self) -> ReloadStats {
        ReloadStats {
            total_reloads: self.total_reloads.load(Ordering::Relaxed),
            successful_reloads: self.successful_reloads.load(Ordering::Relaxed),
            failed_reloads: self.failed_reloads.load(Ordering::Relaxed),
            last_reload_timestamp: self.last_reload_timestamp.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReloadStats {
    pub total_reloads: u64,
    pub successful_reloads: u64,
    pub failed_reloads: u64,
    pub last_reload_timestamp: u64,
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

    /// Hot-reload configuration
    hot_reload_config: HotReloadConfig,

    /// Reload metrics
    reload_metrics: ReloadMetrics,
}

impl PythonStrategyRegistry {
    pub fn new(strategy_dir: PathBuf) -> Result<Self, String> {
        Self::with_hot_reload_config(strategy_dir, HotReloadConfig::default())
    }

    pub fn with_hot_reload_config(strategy_dir: PathBuf, hot_reload_config: HotReloadConfig) -> Result<Self, String> {
        Ok(Self {
            configs: Arc::new(RwLock::new(HashMap::new())),
            loaded_strategies: Arc::new(ParkingLotRwLock::new(HashMap::new())),
            _watcher: None,
            strategy_dir,
            hot_reload_config,
            reload_metrics: ReloadMetrics::default(),
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
        let hot_reload_config = self.hot_reload_config.clone();
        let reload_metrics = self.reload_metrics.clone();

        // Spawn background thread for watching with debouncing
        std::thread::spawn(move || {
            let mut pending_paths: HashMap<PathBuf, std::time::Instant> = HashMap::new();
            let debounce_duration = Duration::from_millis(hot_reload_config.debounce_ms);

            loop {
                // Check for new events (non-blocking with timeout)
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(Ok(event)) => {
                        if matches!(event.kind, EventKind::Modify(_)) {
                            // Add/update pending paths with current timestamp
                            for path in event.paths {
                                pending_paths.insert(path, std::time::Instant::now());
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        eprintln!("⚠ Watch error: {}", e);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        // No events, continue to check pending
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        eprintln!("✗ Watch channel disconnected");
                        break;
                    }
                }

                // Process pending paths that have exceeded debounce duration
                let now = std::time::Instant::now();
                let mut to_process = Vec::new();

                pending_paths.retain(|path, timestamp| {
                    if now.duration_since(*timestamp) >= debounce_duration {
                        to_process.push(path.clone());
                        false // Remove from pending
                    } else {
                        true // Keep pending
                    }
                });

                // Handle file changes after debounce
                for path in to_process {
                    Self::handle_file_change_atomic(
                        &path,
                        &loaded_strategies,
                        &configs,
                        &hot_reload_config,
                        &reload_metrics,
                    );
                }
            }
        });

        self._watcher = Some(Box::new(watcher));
        println!("✓ Hot-reload enabled for {:?}", self.strategy_dir);
        println!("  Debounce: {}ms", self.hot_reload_config.debounce_ms);
        println!("  Skip hash verification: {}", self.hot_reload_config.skip_hash_verification);
        Ok(())
    }

    /// Atomic hot-reload with validation before cache invalidation
    fn handle_file_change_atomic(
        path: &PathBuf,
        cache: &Arc<ParkingLotRwLock<HashMap<String, Arc<PythonStrategy>>>>,
        configs: &Arc<RwLock<HashMap<String, StrategyConfig>>>,
        hot_reload_config: &HotReloadConfig,
        reload_metrics: &ReloadMetrics,
    ) {
        println!("📝 File changed: {:?}", path);

        // Find which strategy config matches this path
        let configs_read = match configs.read() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("✗ Failed to acquire config lock: {}", e);
                reload_metrics.record_failure();
                return;
            }
        };

        let matching_configs: Vec<(String, StrategyConfig)> = configs_read
            .iter()
            .filter(|(_, config)| config.file_path == *path)
            .map(|(id, config)| (id.clone(), config.clone()))
            .collect();

        drop(configs_read);

        if matching_configs.is_empty() {
            println!("  ⓘ No registered strategies match this file (ignoring)");
            return;
        }

        // Attempt to reload each matching strategy
        for (id, config) in matching_configs {
            if !config.enabled {
                println!("  ⓘ Strategy '{}' is disabled (skipping reload)", id);
                continue;
            }

            println!("  🔄 Reloading strategy: {}", id);

            // Verify hash if required (skip in dev mode if configured)
            if !hot_reload_config.skip_hash_verification {
                if let Some(expected_hash) = &config.sha256 {
                    if let Err(e) = verify_strategy_hash(&config.file_path, expected_hash) {
                        eprintln!("  ✗ Hash verification failed for '{}': {}", id, e);
                        eprintln!("    Tip: Set hot_reload.skip_hash_verification = true in dev mode");
                        reload_metrics.record_failure();
                        continue;
                    }
                }
            } else {
                println!("  ⓘ Hash verification skipped (dev mode)");
            }

            // Try to load the new version
            match PythonStrategy::from_file(
                config.file_path.to_str().unwrap(),
                &config.class_name
            ) {
                Ok(new_strategy) => {
                    // Success! Now safely replace in cache
                    let mut cache_write = cache.write();
                    cache_write.insert(id.clone(), Arc::new(new_strategy));
                    drop(cache_write);

                    println!("  ✓ Successfully reloaded strategy: {}", id);
                    reload_metrics.record_success();
                }
                Err(e) => {
                    eprintln!("  ✗ Failed to reload strategy '{}': {}", id, e);
                    eprintln!("    Keeping old version in cache");
                    reload_metrics.record_failure();
                }
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

    /// Get reload metrics
    pub fn get_reload_metrics(&self) -> ReloadStats {
        self.reload_metrics.get_stats()
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

    fn on_bar_data(&mut self, bar_data: &crate::data::types::BarData, bars: &mut BarsContext) -> super::base::Signal {
        // We need mutable access but only have Arc
        // PythonStrategy uses internal Mutex for state, so this is safe
        unsafe {
            let ptr = Arc::as_ptr(&self.inner) as *mut PythonStrategy;
            (*ptr).on_bar_data(bar_data, bars)
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

    fn bar_data_mode(&self) -> crate::data::types::BarDataMode {
        self.inner.bar_data_mode()
    }

    fn preferred_bar_type(&self) -> crate::data::types::BarType {
        self.inner.preferred_bar_type()
    }

    fn max_bars_lookback(&self) -> MaximumBarsLookBack {
        self.inner.max_bars_lookback()
    }
}
