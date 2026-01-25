//! Configuration service for loading, saving, and managing configuration.
//!
//! The ConfigService implements a hybrid approach:
//! - TOML files in `config/` provide defaults (git-tracked)
//! - JSON file in user config directory stores overrides (user-specific)
//! - Runtime config is the merge of defaults + user overrides
//!
//! # Example
//!
//! ```no_run
//! use trading_common::config::{ConfigService, AppConfig};
//!
//! // Load configuration
//! let service = ConfigService::new("config/development.toml").unwrap();
//!
//! // Get merged configuration
//! let config = service.get_config();
//!
//! // Update a section
//! service.update_section("symbols", serde_json::json!(["BTCUSDT", "ETHUSDT"])).unwrap();
//!
//! // Save user overrides
//! service.save().unwrap();
//! ```

use crate::config::schema::*;
use crate::config::validation::{ConfigValidator, ValidationResult};
use parking_lot::RwLock;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Error type for configuration operations.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    ReadError(String),

    #[error("Failed to parse TOML: {0}")]
    TomlParseError(String),

    #[error("Failed to parse JSON: {0}")]
    JsonParseError(String),

    #[error("Failed to write config: {0}")]
    WriteError(String),

    #[error("Section not found: {0}")]
    SectionNotFound(String),

    #[error("Validation failed: {0}")]
    ValidationError(String),

    #[error("Rate limited: please wait {0:?}")]
    RateLimited(Duration),

    #[error("Sensitive section not accessible: {0}")]
    SensitiveSection(String),
}

/// Audit log entry for configuration changes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConfigAuditEntry {
    /// Timestamp of the change
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Section that was changed
    pub section: String,
    /// Action performed
    pub action: ConfigAction,
    /// Previous value (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_value: Option<serde_json::Value>,
    /// New value (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_value: Option<serde_json::Value>,
}

/// Action performed on configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigAction {
    Update,
    Add,
    Remove,
    Reset,
}

/// Configuration service for managing application configuration.
///
/// Implements the hybrid TOML+JSON approach:
/// - Loads defaults from TOML
/// - Loads user overrides from JSON
/// - Merges them with user overrides taking precedence
/// - Provides CRUD operations for sections and array items
pub struct ConfigService {
    /// Path to the defaults TOML file
    defaults_path: PathBuf,
    /// Path to user overrides JSON file
    user_config_path: PathBuf,
    /// Default configuration (from TOML)
    defaults: Arc<RwLock<AppConfig>>,
    /// User overrides (from JSON)
    user_overrides: Arc<RwLock<serde_json::Value>>,
    /// Merged configuration
    merged: Arc<RwLock<AppConfig>>,
    /// Rate limiting: last save time
    last_save: Arc<RwLock<Option<Instant>>>,
    /// Minimum interval between saves (1 second default)
    save_rate_limit: Duration,
    /// Audit log
    audit_log: Arc<RwLock<Vec<ConfigAuditEntry>>>,
    /// Maximum audit log entries
    max_audit_entries: usize,
}

impl ConfigService {
    /// Create a new ConfigService with the given defaults path.
    ///
    /// User config will be stored in `~/.config/rust-trade/user_config.json`
    /// or the Tauri app data directory if available.
    pub fn new<P: AsRef<Path>>(defaults_path: P) -> Result<Self, ConfigError> {
        let defaults_path = defaults_path.as_ref().to_path_buf();
        let user_config_path = Self::get_user_config_path();

        Self::with_paths(defaults_path, user_config_path)
    }

    /// Create a new ConfigService with explicit paths.
    pub fn with_paths(defaults_path: PathBuf, user_config_path: PathBuf) -> Result<Self, ConfigError> {
        // Load defaults from TOML
        let defaults = Self::load_toml_defaults(&defaults_path)?;

        // Load user overrides from JSON (if exists)
        let user_overrides = Self::load_user_overrides(&user_config_path)?;

        // Merge configs
        let merged = Self::merge_configs(&defaults, &user_overrides)?;

        Ok(Self {
            defaults_path,
            user_config_path,
            defaults: Arc::new(RwLock::new(defaults)),
            user_overrides: Arc::new(RwLock::new(user_overrides)),
            merged: Arc::new(RwLock::new(merged)),
            last_save: Arc::new(RwLock::new(None)),
            save_rate_limit: Duration::from_secs(1),
            audit_log: Arc::new(RwLock::new(Vec::new())),
            max_audit_entries: 1000,
        })
    }

    /// Create a new ConfigService with in-memory defaults (for testing).
    pub fn with_defaults(defaults: AppConfig) -> Self {
        let user_overrides = serde_json::json!({});
        let merged = defaults.clone();

        Self {
            defaults_path: PathBuf::from("memory://defaults"),
            user_config_path: PathBuf::from("memory://user"),
            defaults: Arc::new(RwLock::new(defaults)),
            user_overrides: Arc::new(RwLock::new(user_overrides)),
            merged: Arc::new(RwLock::new(merged)),
            last_save: Arc::new(RwLock::new(None)),
            save_rate_limit: Duration::from_secs(1),
            audit_log: Arc::new(RwLock::new(Vec::new())),
            max_audit_entries: 1000,
        }
    }

    /// Get the user config directory path.
    fn get_user_config_path() -> PathBuf {
        // Try standard XDG config directory first
        if let Some(config_dir) = dirs::config_dir() {
            let app_dir = config_dir.join("rust-trade");
            return app_dir.join("user_config.json");
        }

        // Fallback to home directory
        if let Some(home) = dirs::home_dir() {
            return home.join(".rust-trade").join("user_config.json");
        }

        // Last resort: current directory
        PathBuf::from("user_config.json")
    }

    /// Load TOML defaults.
    fn load_toml_defaults(path: &Path) -> Result<AppConfig, ConfigError> {
        if !path.exists() {
            info!("Defaults file not found, using built-in defaults");
            return Ok(AppConfig::default());
        }

        let content = fs::read_to_string(path)
            .map_err(|e| ConfigError::ReadError(format!("{}: {}", path.display(), e)))?;

        toml::from_str(&content)
            .map_err(|e| ConfigError::TomlParseError(e.to_string()))
    }

    /// Load user overrides from JSON.
    fn load_user_overrides(path: &Path) -> Result<serde_json::Value, ConfigError> {
        if !path.exists() {
            debug!("User config not found at {}, starting fresh", path.display());
            return Ok(serde_json::json!({}));
        }

        let content = fs::read_to_string(path)
            .map_err(|e| ConfigError::ReadError(format!("{}: {}", path.display(), e)))?;

        serde_json::from_str(&content)
            .map_err(|e| ConfigError::JsonParseError(e.to_string()))
    }

    /// Merge defaults with user overrides.
    fn merge_configs(
        defaults: &AppConfig,
        overrides: &serde_json::Value,
    ) -> Result<AppConfig, ConfigError> {
        // Convert defaults to JSON
        let mut merged =
            serde_json::to_value(defaults).map_err(|e| ConfigError::JsonParseError(e.to_string()))?;

        // Deep merge overrides into defaults
        Self::deep_merge(&mut merged, overrides);

        // Convert back to AppConfig
        serde_json::from_value(merged).map_err(|e| ConfigError::JsonParseError(e.to_string()))
    }

    /// Deep merge two JSON values.
    fn deep_merge(target: &mut serde_json::Value, source: &serde_json::Value) {
        match (target, source) {
            (serde_json::Value::Object(target_map), serde_json::Value::Object(source_map)) => {
                for (key, source_val) in source_map {
                    if let Some(target_val) = target_map.get_mut(key) {
                        Self::deep_merge(target_val, source_val);
                    } else {
                        target_map.insert(key.clone(), source_val.clone());
                    }
                }
            }
            (target, source) => {
                *target = source.clone();
            }
        }
    }

    /// Get the merged configuration.
    pub fn get_config(&self) -> AppConfig {
        self.merged.read().clone()
    }

    /// Get a specific section of the configuration.
    pub fn get_section(&self, section: &str) -> Result<serde_json::Value, ConfigError> {
        // Check for sensitive sections
        if is_sensitive_section(section) {
            return Err(ConfigError::SensitiveSection(section.to_string()));
        }

        let config = self.merged.read();
        config.get_section(section).map_err(|e| ConfigError::SectionNotFound(e))
    }

    /// Update a configuration section.
    ///
    /// The change is applied to user overrides and persisted.
    pub fn update_section(
        &self,
        section: &str,
        value: serde_json::Value,
    ) -> Result<(), ConfigError> {
        // Check for sensitive sections
        if is_sensitive_section(section) {
            return Err(ConfigError::SensitiveSection(section.to_string()));
        }

        // Validate the new value
        let validation = ConfigValidator::validate_section(section, &value);
        if !validation.valid {
            let errors: Vec<String> = validation.errors.iter().map(|e| e.message.clone()).collect();
            return Err(ConfigError::ValidationError(errors.join("; ")));
        }

        // Get old value for audit
        let old_value = self.get_section(section).ok();

        // Update user overrides
        {
            let mut overrides = self.user_overrides.write();
            Self::set_nested(&mut overrides, section, value.clone())?;
        }

        // Re-merge configuration
        self.refresh_merged()?;

        // Add audit entry
        self.add_audit_entry(ConfigAuditEntry {
            timestamp: chrono::Utc::now(),
            section: section.to_string(),
            action: ConfigAction::Update,
            old_value,
            new_value: Some(value),
        });

        Ok(())
    }

    /// Add an item to an array section.
    pub fn add_item(&self, section: &str, item: serde_json::Value) -> Result<(), ConfigError> {
        // Check for sensitive sections
        if is_sensitive_section(section) {
            return Err(ConfigError::SensitiveSection(section.to_string()));
        }

        // Get current array from merged config
        let current = self.get_section(section)?;
        if !current.is_array() {
            return Err(ConfigError::SectionNotFound(format!(
                "{} is not an array",
                section
            )));
        }

        // Create new array with item added
        let mut arr = current.as_array().unwrap().clone();
        arr.push(item.clone());

        // Update the section
        self.update_section(section, serde_json::Value::Array(arr))?;

        // Update audit to show add action
        if let Some(entry) = self.audit_log.write().last_mut() {
            entry.action = ConfigAction::Add;
        }

        Ok(())
    }

    /// Remove an item from an array section by index.
    pub fn remove_item(&self, section: &str, index: usize) -> Result<(), ConfigError> {
        // Check for sensitive sections
        if is_sensitive_section(section) {
            return Err(ConfigError::SensitiveSection(section.to_string()));
        }

        // Get current array from merged config
        let current = self.get_section(section)?;
        if !current.is_array() {
            return Err(ConfigError::SectionNotFound(format!(
                "{} is not an array",
                section
            )));
        }

        let mut arr = current.as_array().unwrap().clone();
        if index >= arr.len() {
            return Err(ConfigError::SectionNotFound(format!(
                "Index {} out of bounds for array of length {}",
                index,
                arr.len()
            )));
        }

        // Get removed value for audit
        let removed = arr.remove(index);

        // Update the section
        self.update_section(section, serde_json::Value::Array(arr))?;

        // Update audit to show remove action
        if let Some(entry) = self.audit_log.write().last_mut() {
            entry.action = ConfigAction::Remove;
            entry.old_value = Some(removed);
            entry.new_value = None;
        }

        Ok(())
    }

    /// Update an item in an array section by index.
    pub fn update_item(
        &self,
        section: &str,
        index: usize,
        item: serde_json::Value,
    ) -> Result<(), ConfigError> {
        // Check for sensitive sections
        if is_sensitive_section(section) {
            return Err(ConfigError::SensitiveSection(section.to_string()));
        }

        // Get current array from merged config
        let current = self.get_section(section)?;
        if !current.is_array() {
            return Err(ConfigError::SectionNotFound(format!(
                "{} is not an array",
                section
            )));
        }

        let mut arr = current.as_array().unwrap().clone();
        if index >= arr.len() {
            return Err(ConfigError::SectionNotFound(format!(
                "Index {} out of bounds for array of length {}",
                index,
                arr.len()
            )));
        }

        arr[index] = item;
        self.update_section(section, serde_json::Value::Array(arr))
    }

    /// Reset a section to its default value.
    pub fn reset_section(&self, section: &str) -> Result<(), ConfigError> {
        // Check for sensitive sections
        if is_sensitive_section(section) {
            return Err(ConfigError::SensitiveSection(section.to_string()));
        }

        // Get old value for audit
        let old_value = self.get_section(section).ok();

        // Remove section from user overrides
        {
            let mut overrides = self.user_overrides.write();
            Self::remove_nested(&mut overrides, section);
        }

        // Re-merge configuration
        self.refresh_merged()?;

        // Add audit entry
        self.add_audit_entry(ConfigAuditEntry {
            timestamp: chrono::Utc::now(),
            section: section.to_string(),
            action: ConfigAction::Reset,
            old_value,
            new_value: None,
        });

        Ok(())
    }

    /// Validate the entire current configuration.
    pub fn validate(&self) -> ValidationResult {
        let config = self.merged.read();
        ConfigValidator::validate(&config)
    }

    /// Validate a specific value for a section.
    pub fn validate_section(&self, section: &str, value: &serde_json::Value) -> ValidationResult {
        ConfigValidator::validate_section(section, value)
    }

    /// Save user overrides to disk.
    pub fn save(&self) -> Result<(), ConfigError> {
        // Rate limiting check
        {
            let mut last_save = self.last_save.write();
            if let Some(last) = *last_save {
                let elapsed = last.elapsed();
                if elapsed < self.save_rate_limit {
                    let remaining = self.save_rate_limit - elapsed;
                    return Err(ConfigError::RateLimited(remaining));
                }
            }
            *last_save = Some(Instant::now());
        }

        // Only save if not using memory paths
        if self.user_config_path.to_string_lossy().starts_with("memory://") {
            debug!("Skipping save for in-memory config");
            return Ok(());
        }

        // Ensure directory exists
        if let Some(parent) = self.user_config_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| ConfigError::WriteError(format!("Failed to create directory: {}", e)))?;
        }

        // Write JSON
        let overrides = self.user_overrides.read();
        let json = serde_json::to_string_pretty(&*overrides)
            .map_err(|e| ConfigError::WriteError(e.to_string()))?;

        fs::write(&self.user_config_path, json)
            .map_err(|e| ConfigError::WriteError(format!("{}: {}", self.user_config_path.display(), e)))?;

        info!("Saved user config to {}", self.user_config_path.display());
        Ok(())
    }

    /// Reload configuration from disk.
    pub fn reload(&self) -> Result<(), ConfigError> {
        // Reload defaults
        let new_defaults = Self::load_toml_defaults(&self.defaults_path)?;
        *self.defaults.write() = new_defaults;

        // Reload user overrides
        let new_overrides = Self::load_user_overrides(&self.user_config_path)?;
        *self.user_overrides.write() = new_overrides;

        // Re-merge
        self.refresh_merged()?;

        info!("Configuration reloaded");
        Ok(())
    }

    /// Get the audit log.
    pub fn get_audit_log(&self) -> Vec<ConfigAuditEntry> {
        self.audit_log.read().clone()
    }

    /// Clear the audit log.
    pub fn clear_audit_log(&self) {
        self.audit_log.write().clear();
    }

    /// Get user overrides as JSON (for debugging).
    pub fn get_overrides(&self) -> serde_json::Value {
        self.user_overrides.read().clone()
    }

    /// Check if there are any user overrides.
    pub fn has_overrides(&self) -> bool {
        let overrides = self.user_overrides.read();
        if let Some(obj) = overrides.as_object() {
            !obj.is_empty()
        } else {
            false
        }
    }

    // Internal helpers

    fn refresh_merged(&self) -> Result<(), ConfigError> {
        let defaults = self.defaults.read();
        let overrides = self.user_overrides.read();
        let merged = Self::merge_configs(&defaults, &overrides)?;
        *self.merged.write() = merged;
        Ok(())
    }

    fn set_nested(
        target: &mut serde_json::Value,
        path: &str,
        value: serde_json::Value,
    ) -> Result<(), ConfigError> {
        let parts: Vec<&str> = path.split('.').collect();

        if parts.is_empty() {
            return Err(ConfigError::SectionNotFound("Empty path".to_string()));
        }

        // Ensure target is an object
        if !target.is_object() {
            *target = serde_json::json!({});
        }

        let mut current = target;
        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                // Last part: set the value
                current[*part] = value;
                return Ok(());
            } else {
                // Intermediate part: ensure object exists
                if !current.get(*part).map(|v| v.is_object()).unwrap_or(false) {
                    current[*part] = serde_json::json!({});
                }
                current = current.get_mut(*part).unwrap();
            }
        }

        Ok(())
    }

    fn remove_nested(target: &mut serde_json::Value, path: &str) {
        let parts: Vec<&str> = path.split('.').collect();

        if parts.is_empty() {
            return;
        }

        if parts.len() == 1 {
            if let Some(obj) = target.as_object_mut() {
                obj.remove(parts[0]);
            }
            return;
        }

        // Navigate to parent
        let mut current = target;
        for part in &parts[..parts.len() - 1] {
            if let Some(next) = current.get_mut(*part) {
                current = next;
            } else {
                return; // Path doesn't exist
            }
        }

        // Remove the leaf
        if let Some(obj) = current.as_object_mut() {
            obj.remove(*parts.last().unwrap());
        }
    }

    fn add_audit_entry(&self, entry: ConfigAuditEntry) {
        let mut log = self.audit_log.write();
        log.push(entry);

        // Trim if too large
        if log.len() > self.max_audit_entries {
            let drain_count = log.len() - self.max_audit_entries;
            log.drain(0..drain_count);
        }
    }
}

// Provide a thread-safe wrapper
impl Clone for ConfigService {
    fn clone(&self) -> Self {
        Self {
            defaults_path: self.defaults_path.clone(),
            user_config_path: self.user_config_path.clone(),
            defaults: Arc::clone(&self.defaults),
            user_overrides: Arc::clone(&self.user_overrides),
            merged: Arc::clone(&self.merged),
            last_save: Arc::clone(&self.last_save),
            save_rate_limit: self.save_rate_limit,
            audit_log: Arc::clone(&self.audit_log),
            max_audit_entries: self.max_audit_entries,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_service() -> ConfigService {
        ConfigService::with_defaults(AppConfig::default())
    }

    #[test]
    fn test_config_service_creation() {
        let service = create_test_service();
        let config = service.get_config();
        assert_eq!(config.accounts.default.id, "DEFAULT");
    }

    #[test]
    fn test_get_section() {
        let service = create_test_service();
        let section = service.get_section("accounts.default").unwrap();
        assert_eq!(section["id"], "DEFAULT");
    }

    #[test]
    fn test_get_section_not_found() {
        let service = create_test_service();
        let result = service.get_section("nonexistent.section");
        assert!(result.is_err());
    }

    #[test]
    fn test_update_section() {
        let service = create_test_service();
        let new_symbols = serde_json::json!(["BTCUSDT", "ETHUSDT"]);
        service.update_section("symbols", new_symbols).unwrap();

        let config = service.get_config();
        assert_eq!(config.symbols.len(), 2);
        assert_eq!(config.symbols[0], "BTCUSDT");
    }

    #[test]
    fn test_update_nested_section() {
        let service = create_test_service();
        let new_default = serde_json::json!({
            "id": "MAIN",
            "currency": "USDC",
            "initial_balance": "50000"
        });
        service.update_section("accounts.default", new_default).unwrap();

        let config = service.get_config();
        assert_eq!(config.accounts.default.id, "MAIN");
        assert_eq!(config.accounts.default.currency, "USDC");
    }

    #[test]
    fn test_add_item() {
        let service = create_test_service();

        // First set up symbols
        service
            .update_section("symbols", serde_json::json!(["BTCUSDT"]))
            .unwrap();

        // Add another symbol
        service
            .add_item("symbols", serde_json::json!("ETHUSDT"))
            .unwrap();

        let config = service.get_config();
        assert_eq!(config.symbols.len(), 2);
        assert_eq!(config.symbols[1], "ETHUSDT");
    }

    #[test]
    fn test_remove_item() {
        let service = create_test_service();

        // Set up symbols
        service
            .update_section("symbols", serde_json::json!(["BTCUSDT", "ETHUSDT", "SOLUSDT"]))
            .unwrap();

        // Remove middle item
        service.remove_item("symbols", 1).unwrap();

        let config = service.get_config();
        assert_eq!(config.symbols.len(), 2);
        assert_eq!(config.symbols[0], "BTCUSDT");
        assert_eq!(config.symbols[1], "SOLUSDT");
    }

    #[test]
    fn test_update_item() {
        let service = create_test_service();

        // Set up symbols
        service
            .update_section("symbols", serde_json::json!(["BTCUSDT", "ETHUSDT"]))
            .unwrap();

        // Update item
        service
            .update_item("symbols", 1, serde_json::json!("SOLUSDT"))
            .unwrap();

        let config = service.get_config();
        assert_eq!(config.symbols[1], "SOLUSDT");
    }

    #[test]
    fn test_reset_section() {
        let service = create_test_service();

        // Update symbols
        service
            .update_section("symbols", serde_json::json!(["BTCUSDT"]))
            .unwrap();

        // Reset to default
        service.reset_section("symbols").unwrap();

        let config = service.get_config();
        assert!(config.symbols.is_empty()); // Default is empty
    }

    #[test]
    fn test_validation_on_update() {
        let service = create_test_service();

        // Try to update with invalid symbols (lowercase)
        let result = service.update_section("symbols", serde_json::json!(["btcusdt"]));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("uppercase"));
    }

    #[test]
    fn test_sensitive_section_blocked() {
        let service = create_test_service();

        let result = service.get_section("database.url");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::SensitiveSection(_)));

        let result = service.update_section("server.host", serde_json::json!("evil.com"));
        assert!(result.is_err());
    }

    #[test]
    fn test_audit_log() {
        let service = create_test_service();

        service
            .update_section("symbols", serde_json::json!(["BTCUSDT"]))
            .unwrap();

        let log = service.get_audit_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].section, "symbols");
        assert!(matches!(log[0].action, ConfigAction::Update));
    }

    #[test]
    fn test_has_overrides() {
        let service = create_test_service();

        assert!(!service.has_overrides());

        service
            .update_section("symbols", serde_json::json!(["BTCUSDT"]))
            .unwrap();

        assert!(service.has_overrides());
    }

    #[test]
    fn test_validate() {
        let service = create_test_service();
        let result = service.validate();
        assert!(result.valid);
    }

    #[test]
    fn test_deep_merge() {
        let mut target = serde_json::json!({
            "a": {
                "b": 1,
                "c": 2
            },
            "d": 3
        });

        let source = serde_json::json!({
            "a": {
                "b": 10,
                "e": 5
            },
            "f": 6
        });

        ConfigService::deep_merge(&mut target, &source);

        assert_eq!(target["a"]["b"], 10); // Overwritten
        assert_eq!(target["a"]["c"], 2); // Preserved
        assert_eq!(target["a"]["e"], 5); // Added
        assert_eq!(target["d"], 3); // Preserved
        assert_eq!(target["f"], 6); // Added
    }

    #[test]
    fn test_clone() {
        let service1 = create_test_service();
        let service2 = service1.clone();

        // Update through one service
        service1
            .update_section("symbols", serde_json::json!(["BTCUSDT"]))
            .unwrap();

        // Both should see the change (they share state)
        let config1 = service1.get_config();
        let config2 = service2.get_config();
        assert_eq!(config1.symbols, config2.symbols);
    }

    #[test]
    fn test_add_simulation_account() {
        let service = create_test_service();

        let new_account = serde_json::json!({
            "id": "SIM-001",
            "currency": "USDT",
            "initial_balance": "50000",
            "strategies": ["rsi"]
        });

        service.add_item("accounts.simulation", new_account).unwrap();

        let config = service.get_config();
        assert_eq!(config.accounts.simulation.len(), 1);
        assert_eq!(config.accounts.simulation[0].id, "SIM-001");
    }

    #[test]
    fn test_update_rsi_strategy_params() {
        let service = create_test_service();

        let new_rsi = serde_json::json!({
            "period": 21,
            "oversold": 25,
            "overbought": 75,
            "order_quantity": "200",
            "lookback_multiplier": 2.5,
            "lookback_buffer": 15
        });

        service.update_section("builtin_strategies.rsi", new_rsi).unwrap();

        let config = service.get_config();
        assert_eq!(config.builtin_strategies.rsi.period, 21);
        assert_eq!(config.builtin_strategies.rsi.oversold, 25);
        assert_eq!(config.builtin_strategies.rsi.overbought, 75);
    }

    #[test]
    fn test_remove_item_out_of_bounds() {
        let service = create_test_service();

        service
            .update_section("symbols", serde_json::json!(["BTCUSDT"]))
            .unwrap();

        let result = service.remove_item("symbols", 5);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("out of bounds"));
    }

    #[test]
    fn test_add_item_to_non_array() {
        let service = create_test_service();

        let result = service.add_item("accounts.default", serde_json::json!("item"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not an array"));
    }
}
