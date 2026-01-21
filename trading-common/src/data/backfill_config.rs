//! Backfill configuration structures
//!
//! These configuration structs define safe defaults for backfill behavior.
//! All cost-incurring features are disabled by default.

use serde::{Deserialize, Serialize};

/// Backfill operational mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BackfillMode {
    /// Backfill completely disabled (default)
    #[default]
    Disabled,
    /// Trigger backfill automatically when data is missing
    OnDemand,
    /// Only fill gaps via scheduled jobs
    Scheduled,
    /// Only allow manual backfill via CLI
    ManualOnly,
}

impl std::fmt::Display for BackfillMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackfillMode::Disabled => write!(f, "disabled"),
            BackfillMode::OnDemand => write!(f, "on_demand"),
            BackfillMode::Scheduled => write!(f, "scheduled"),
            BackfillMode::ManualOnly => write!(f, "manual_only"),
        }
    }
}

/// Main backfill configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillConfig {
    /// Master switch - must be explicitly enabled
    #[serde(default)]
    pub enabled: bool,

    /// Operational mode
    #[serde(default)]
    pub mode: BackfillMode,

    /// Cost limits configuration
    #[serde(default)]
    pub cost_limits: CostLimitsConfig,

    /// On-demand backfill configuration
    #[serde(default)]
    pub on_demand: OnDemandConfig,

    /// Scheduled backfill configuration
    #[serde(default)]
    pub scheduled: ScheduledConfig,

    /// Provider configuration
    #[serde(default)]
    pub provider: ProviderConfig,
}

impl Default for BackfillConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: BackfillMode::Disabled,
            cost_limits: CostLimitsConfig::default(),
            on_demand: OnDemandConfig::default(),
            scheduled: ScheduledConfig::default(),
            provider: ProviderConfig::default(),
        }
    }
}

impl BackfillConfig {
    /// Check if backfill is effectively enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled && self.mode != BackfillMode::Disabled
    }

    /// Check if on-demand backfill is enabled
    pub fn is_on_demand_enabled(&self) -> bool {
        self.enabled && self.mode == BackfillMode::OnDemand && self.on_demand.enabled
    }

    /// Check if scheduled backfill is enabled
    pub fn is_scheduled_enabled(&self) -> bool {
        self.enabled
            && (self.mode == BackfillMode::Scheduled || self.mode == BackfillMode::OnDemand)
            && self.scheduled.enabled
    }

    /// Check if manual backfill is allowed
    pub fn is_manual_allowed(&self) -> bool {
        self.enabled && self.mode != BackfillMode::Disabled
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.enabled && self.mode == BackfillMode::Disabled {
            return Err("Backfill enabled but mode is 'disabled'".to_string());
        }

        self.cost_limits.validate()?;
        self.on_demand.validate()?;

        Ok(())
    }
}

/// Cost limits configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostLimitsConfig {
    /// Maximum cost per single request (USD)
    #[serde(default = "default_max_per_request")]
    pub max_cost_per_request: f64,

    /// Maximum daily spend (USD)
    #[serde(default = "default_max_daily")]
    pub max_daily_spend: f64,

    /// Maximum monthly spend (USD)
    #[serde(default = "default_max_monthly")]
    pub max_monthly_spend: f64,

    /// Require confirmation above this threshold (USD)
    #[serde(default = "default_confirmation_threshold")]
    pub confirmation_threshold: f64,

    /// Auto-approve requests below this threshold (USD)
    #[serde(default = "default_auto_approve_threshold")]
    pub auto_approve_threshold: f64,

    /// Enable auto-approval (disabled by default)
    #[serde(default)]
    pub auto_approve_enabled: bool,
}

fn default_max_per_request() -> f64 {
    10.0
}

fn default_max_daily() -> f64 {
    50.0
}

fn default_max_monthly() -> f64 {
    500.0
}

fn default_confirmation_threshold() -> f64 {
    5.0
}

fn default_auto_approve_threshold() -> f64 {
    1.0
}

impl Default for CostLimitsConfig {
    fn default() -> Self {
        Self {
            max_cost_per_request: default_max_per_request(),
            max_daily_spend: default_max_daily(),
            max_monthly_spend: default_max_monthly(),
            confirmation_threshold: default_confirmation_threshold(),
            auto_approve_threshold: default_auto_approve_threshold(),
            auto_approve_enabled: false,
        }
    }
}

impl CostLimitsConfig {
    /// Check if a cost would be auto-approved
    pub fn can_auto_approve(&self, cost: f64) -> bool {
        self.auto_approve_enabled && cost <= self.auto_approve_threshold
    }

    /// Check if a cost requires confirmation
    pub fn requires_confirmation(&self, cost: f64) -> bool {
        !self.can_auto_approve(cost) && cost > self.confirmation_threshold
    }

    /// Check if a cost exceeds per-request limit
    pub fn exceeds_per_request_limit(&self, cost: f64) -> bool {
        cost > self.max_cost_per_request
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.max_cost_per_request <= 0.0 {
            return Err("max_cost_per_request must be positive".to_string());
        }
        if self.max_daily_spend <= 0.0 {
            return Err("max_daily_spend must be positive".to_string());
        }
        if self.max_monthly_spend <= 0.0 {
            return Err("max_monthly_spend must be positive".to_string());
        }
        if self.max_daily_spend > self.max_monthly_spend {
            return Err("max_daily_spend cannot exceed max_monthly_spend".to_string());
        }
        if self.confirmation_threshold < 0.0 {
            return Err("confirmation_threshold cannot be negative".to_string());
        }
        if self.auto_approve_threshold < 0.0 {
            return Err("auto_approve_threshold cannot be negative".to_string());
        }
        if self.auto_approve_enabled && self.auto_approve_threshold > self.confirmation_threshold {
            return Err(
                "auto_approve_threshold should not exceed confirmation_threshold".to_string(),
            );
        }
        Ok(())
    }
}

/// On-demand backfill configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnDemandConfig {
    /// Enable on-demand backfill
    #[serde(default)]
    pub enabled: bool,

    /// Maximum lookback period (days)
    #[serde(default = "default_max_lookback")]
    pub max_lookback_days: u32,

    /// Minimum gap size to trigger backfill (minutes)
    #[serde(default = "default_min_gap")]
    pub min_gap_minutes: u32,

    /// Priority for on-demand jobs
    #[serde(default = "default_on_demand_priority")]
    pub priority: i32,

    /// Cooldown between auto-triggered requests for same symbol (seconds)
    #[serde(default = "default_cooldown")]
    pub cooldown_secs: u64,
}

fn default_max_lookback() -> u32 {
    30
}

fn default_min_gap() -> u32 {
    5
}

fn default_on_demand_priority() -> i32 {
    10
}

fn default_cooldown() -> u64 {
    300 // 5 minutes
}

impl Default for OnDemandConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_lookback_days: default_max_lookback(),
            min_gap_minutes: default_min_gap(),
            priority: default_on_demand_priority(),
            cooldown_secs: default_cooldown(),
        }
    }
}

impl OnDemandConfig {
    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.max_lookback_days == 0 {
            return Err("max_lookback_days must be at least 1".to_string());
        }
        if self.max_lookback_days > 365 {
            return Err("max_lookback_days cannot exceed 365".to_string());
        }
        Ok(())
    }
}

/// Scheduled backfill configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledConfig {
    /// Enable scheduled backfill
    #[serde(default)]
    pub enabled: bool,

    /// Cron expression for gap-filling schedule
    #[serde(default = "default_schedule")]
    pub schedule: String,

    /// Maximum gaps to fill per scheduled run
    #[serde(default = "default_max_gaps")]
    pub max_gaps_per_run: u32,

    /// Priority for scheduled jobs
    #[serde(default)]
    pub priority: i32,
}

fn default_schedule() -> String {
    "0 0 * * *".to_string() // Daily at midnight
}

fn default_max_gaps() -> u32 {
    10
}

impl Default for ScheduledConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            schedule: default_schedule(),
            max_gaps_per_run: default_max_gaps(),
            priority: 0,
        }
    }
}

/// Provider configuration for backfill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Provider name (e.g., "databento")
    #[serde(default = "default_provider")]
    pub name: String,

    /// Default dataset
    #[serde(default = "default_dataset")]
    pub dataset: String,

    /// Maximum concurrent backfill jobs
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_jobs: u32,

    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Retry attempts for failed requests
    #[serde(default = "default_retries")]
    pub retry_attempts: u32,
}

fn default_provider() -> String {
    "databento".to_string()
}

fn default_dataset() -> String {
    "GLBX.MDP3".to_string()
}

fn default_max_concurrent() -> u32 {
    2
}

fn default_timeout() -> u64 {
    300 // 5 minutes
}

fn default_retries() -> u32 {
    3
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            name: default_provider(),
            dataset: default_dataset(),
            max_concurrent_jobs: default_max_concurrent(),
            timeout_secs: default_timeout(),
            retry_attempts: default_retries(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_is_disabled() {
        let config = BackfillConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.mode, BackfillMode::Disabled);
        assert!(!config.is_enabled());
        assert!(!config.is_on_demand_enabled());
    }

    #[test]
    fn test_cost_limits_auto_approve() {
        let mut limits = CostLimitsConfig::default();
        assert!(!limits.can_auto_approve(0.5));

        limits.auto_approve_enabled = true;
        assert!(limits.can_auto_approve(0.5));
        assert!(limits.can_auto_approve(1.0));
        assert!(!limits.can_auto_approve(1.5));
    }

    #[test]
    fn test_cost_limits_validation() {
        let mut limits = CostLimitsConfig::default();
        assert!(limits.validate().is_ok());

        limits.max_daily_spend = 1000.0;
        limits.max_monthly_spend = 500.0;
        assert!(limits.validate().is_err());
    }

    #[test]
    fn test_config_validation() {
        let mut config = BackfillConfig::default();
        assert!(config.validate().is_ok());

        config.enabled = true;
        config.mode = BackfillMode::Disabled;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_backfill_modes() {
        let mut config = BackfillConfig::default();
        config.enabled = true;
        config.on_demand.enabled = true;

        config.mode = BackfillMode::OnDemand;
        assert!(config.is_on_demand_enabled());
        assert!(config.is_manual_allowed());

        config.mode = BackfillMode::ManualOnly;
        assert!(!config.is_on_demand_enabled());
        assert!(config.is_manual_allowed());
    }
}
