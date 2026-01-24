//! Backfill service trait and types
//!
//! This module defines the abstract interface for backfill services.
//! The actual implementation lives in `data-manager` crate to avoid
//! circular dependencies with provider-specific code.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Source that triggered the backfill request
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackfillSource {
    /// Triggered automatically when data was missing during a query
    OnDemand,
    /// Triggered by scheduled gap-filling job
    Scheduled,
    /// Triggered manually via CLI or API
    Manual,
}

impl std::fmt::Display for BackfillSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackfillSource::OnDemand => write!(f, "on_demand"),
            BackfillSource::Scheduled => write!(f, "scheduled"),
            BackfillSource::Manual => write!(f, "manual"),
        }
    }
}

/// A detected gap in data coverage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataGap {
    /// Symbol with missing data
    pub symbol: String,
    /// Start of the gap (inclusive)
    pub start: DateTime<Utc>,
    /// End of the gap (exclusive)
    pub end: DateTime<Utc>,
    /// Estimated missing records
    pub estimated_records: Option<u64>,
    /// Gap duration in minutes
    pub duration_minutes: i64,
}

impl DataGap {
    /// Create a new data gap
    pub fn new(symbol: String, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        let duration_minutes = (end - start).num_minutes();
        Self {
            symbol,
            start,
            end,
            estimated_records: None,
            duration_minutes,
        }
    }

    /// Set estimated records
    pub fn with_estimated_records(mut self, records: u64) -> Self {
        self.estimated_records = Some(records);
        self
    }
}

/// Cost estimate returned before any data fetch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    /// Estimated cost in USD
    pub estimated_cost_usd: f64,
    /// Estimated number of records to fetch
    pub estimated_records: u64,
    /// Whether this exceeds configured cost limits
    pub exceeds_limits: bool,
    /// Warnings about the request (e.g., "near daily limit")
    pub warnings: Vec<String>,
    /// Whether this request can be auto-approved (below threshold)
    pub can_auto_approve: bool,
    /// Reason if cannot auto-approve
    pub auto_approve_reason: Option<String>,
    /// Current daily spend before this request
    pub current_daily_spend: f64,
    /// Current monthly spend before this request
    pub current_monthly_spend: f64,
}

impl CostEstimate {
    /// Create a new cost estimate
    pub fn new(estimated_cost_usd: f64, estimated_records: u64) -> Self {
        Self {
            estimated_cost_usd,
            estimated_records,
            exceeds_limits: false,
            warnings: Vec::new(),
            can_auto_approve: false,
            auto_approve_reason: Some("Auto-approval not enabled".to_string()),
            current_daily_spend: 0.0,
            current_monthly_spend: 0.0,
        }
    }

    /// Mark as exceeding limits
    pub fn with_exceeds_limits(mut self, reason: String) -> Self {
        self.exceeds_limits = true;
        self.warnings.push(reason);
        self
    }

    /// Add a warning
    pub fn with_warning(mut self, warning: String) -> Self {
        self.warnings.push(warning);
        self
    }

    /// Set auto-approve status
    pub fn with_auto_approve(mut self, can_approve: bool, reason: Option<String>) -> Self {
        self.can_auto_approve = can_approve;
        self.auto_approve_reason = reason;
        self
    }

    /// Set current spend
    pub fn with_current_spend(mut self, daily: f64, monthly: f64) -> Self {
        self.current_daily_spend = daily;
        self.current_monthly_spend = monthly;
        self
    }
}

/// Request to backfill data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillRequest {
    /// Unique request ID
    pub id: Uuid,
    /// Symbols to backfill
    pub symbols: Vec<String>,
    /// Start time (inclusive)
    pub start: DateTime<Utc>,
    /// End time (exclusive)
    pub end: DateTime<Utc>,
    /// Source that triggered this request
    pub source: BackfillSource,
    /// Priority (higher = more urgent)
    pub priority: i32,
    /// Provider to use (optional, uses default if not specified)
    pub provider: Option<String>,
    /// Dataset to use (optional, uses provider default)
    pub dataset: Option<String>,
    /// Skip confirmation (for auto-approved requests)
    pub skip_confirmation: bool,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
}

impl BackfillRequest {
    /// Create a new backfill request
    pub fn new(
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        source: BackfillSource,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            symbols: vec![symbol.to_string()],
            start,
            end,
            source,
            priority: match source {
                BackfillSource::OnDemand => 10, // Higher priority for on-demand
                BackfillSource::Manual => 5,    // Medium for manual
                BackfillSource::Scheduled => 0, // Lowest for scheduled
            },
            provider: None,
            dataset: None,
            skip_confirmation: false,
            created_at: Utc::now(),
        }
    }

    /// Create with multiple symbols
    pub fn with_symbols(
        symbols: Vec<String>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        source: BackfillSource,
    ) -> Self {
        let mut req = Self::new(&symbols[0], start, end, source);
        req.symbols = symbols;
        req
    }

    /// Set provider
    pub fn with_provider(mut self, provider: String) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Set dataset
    pub fn with_dataset(mut self, dataset: String) -> Self {
        self.dataset = Some(dataset);
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Allow auto-approval (skip confirmation)
    pub fn with_auto_approve(mut self) -> Self {
        self.skip_confirmation = true;
        self
    }
}

/// Status of a backfill job
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackfillStatus {
    /// Job is queued
    Pending,
    /// Cost estimation in progress
    Estimating,
    /// Waiting for user confirmation
    AwaitingConfirmation,
    /// Data fetch in progress
    Running,
    /// Job completed successfully
    Completed,
    /// Job failed
    Failed,
    /// Job was cancelled
    Cancelled,
}

impl std::fmt::Display for BackfillStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackfillStatus::Pending => write!(f, "pending"),
            BackfillStatus::Estimating => write!(f, "estimating"),
            BackfillStatus::AwaitingConfirmation => write!(f, "awaiting_confirmation"),
            BackfillStatus::Running => write!(f, "running"),
            BackfillStatus::Completed => write!(f, "completed"),
            BackfillStatus::Failed => write!(f, "failed"),
            BackfillStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Result of a completed backfill job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillResult {
    /// Job ID
    pub job_id: Uuid,
    /// Status
    pub status: BackfillStatus,
    /// Records fetched
    pub records_fetched: u64,
    /// Records inserted (may differ due to duplicates)
    pub records_inserted: u64,
    /// Actual cost incurred (USD)
    pub actual_cost_usd: f64,
    /// Duration in seconds
    pub duration_secs: f64,
    /// Error message if failed
    pub error: Option<String>,
    /// Completion timestamp
    pub completed_at: DateTime<Utc>,
}

/// Backfill service errors
#[derive(Error, Debug)]
pub enum BackfillError {
    /// Backfill is disabled in configuration
    #[error("Backfill is disabled. Enable it in configuration to use this feature.")]
    Disabled,

    /// Cost limit would be exceeded
    #[error("Cost limit exceeded: {0}")]
    CostLimitExceeded(String),

    /// Request requires user confirmation
    #[error("Request requires confirmation: estimated cost ${:.2}", .0.estimated_cost_usd)]
    RequiresConfirmation(CostEstimate),

    /// Provider error
    #[error("Provider error: {0}")]
    Provider(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Database error
    #[error("Database error: {0}")]
    Database(String),

    /// Job not found
    #[error("Job not found: {0}")]
    NotFound(Uuid),

    /// Job already exists
    #[error("Duplicate request: similar job already pending")]
    Duplicate,

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

pub type BackfillServiceResult<T> = Result<T, BackfillError>;

/// Trait for backfill service implementations
///
/// This trait is implemented in `data-manager` crate, which has access to
/// provider implementations (Databento, etc). `trading-common` can use this
/// trait without depending on provider-specific code.
#[async_trait]
pub trait BackfillService: Send + Sync {
    /// Check if backfill is enabled in configuration
    fn is_enabled(&self) -> bool;

    /// Detect gaps in data coverage for a symbol
    async fn detect_gaps(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> BackfillServiceResult<Vec<DataGap>>;

    /// Estimate cost for a backfill request
    ///
    /// Returns cost estimate including:
    /// - Estimated cost in USD
    /// - Whether limits would be exceeded
    /// - Whether request can be auto-approved
    async fn estimate_cost(
        &self,
        symbols: &[String],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> BackfillServiceResult<CostEstimate>;

    /// Submit a backfill request
    ///
    /// Returns the job ID. For requests that require confirmation,
    /// returns `BackfillError::RequiresConfirmation` with the cost estimate.
    async fn request_backfill(&self, request: BackfillRequest) -> BackfillServiceResult<Uuid>;

    /// Confirm a pending request (after user approval)
    async fn confirm_request(&self, job_id: Uuid) -> BackfillServiceResult<()>;

    /// Cancel a pending or running request
    async fn cancel_request(&self, job_id: Uuid) -> BackfillServiceResult<()>;

    /// Get status of a backfill job
    async fn get_job_status(&self, job_id: Uuid) -> BackfillServiceResult<BackfillStatus>;

    /// Get result of a completed job
    async fn get_job_result(&self, job_id: Uuid) -> BackfillServiceResult<Option<BackfillResult>>;

    /// Get current spend statistics
    async fn get_spend_stats(&self) -> BackfillServiceResult<SpendStats>;
}

/// Spend statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendStats {
    /// Today's spend in USD
    pub daily_spend: f64,
    /// Daily limit in USD
    pub daily_limit: f64,
    /// Month-to-date spend in USD
    pub monthly_spend: f64,
    /// Monthly limit in USD
    pub monthly_limit: f64,
    /// Remaining daily budget
    pub daily_remaining: f64,
    /// Remaining monthly budget
    pub monthly_remaining: f64,
}

impl SpendStats {
    /// Create new spend stats
    pub fn new(daily_spend: f64, daily_limit: f64, monthly_spend: f64, monthly_limit: f64) -> Self {
        Self {
            daily_spend,
            daily_limit,
            monthly_spend,
            monthly_limit,
            daily_remaining: (daily_limit - daily_spend).max(0.0),
            monthly_remaining: (monthly_limit - monthly_spend).max(0.0),
        }
    }

    /// Check if there's remaining budget
    pub fn has_budget(&self, amount: f64) -> bool {
        amount <= self.daily_remaining && amount <= self.monthly_remaining
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_gap_creation() {
        let start = Utc::now();
        let end = start + chrono::Duration::hours(2);
        let gap = DataGap::new("ES".to_string(), start, end);

        assert_eq!(gap.symbol, "ES");
        assert_eq!(gap.duration_minutes, 120);
        assert!(gap.estimated_records.is_none());
    }

    #[test]
    fn test_cost_estimate_builder() {
        let estimate = CostEstimate::new(5.50, 10000)
            .with_warning("Near daily limit".to_string())
            .with_auto_approve(true, None)
            .with_current_spend(45.0, 400.0);

        assert_eq!(estimate.estimated_cost_usd, 5.50);
        assert_eq!(estimate.estimated_records, 10000);
        assert!(estimate.can_auto_approve);
        assert_eq!(estimate.warnings.len(), 1);
        assert_eq!(estimate.current_daily_spend, 45.0);
    }

    #[test]
    fn test_backfill_request_creation() {
        let start = Utc::now();
        let end = start + chrono::Duration::days(1);
        let request = BackfillRequest::new("ES", start, end, BackfillSource::OnDemand);

        assert_eq!(request.symbols, vec!["ES"]);
        assert_eq!(request.priority, 10); // OnDemand has high priority
        assert!(!request.skip_confirmation);
    }

    #[test]
    fn test_spend_stats() {
        let stats = SpendStats::new(30.0, 50.0, 400.0, 500.0);

        assert_eq!(stats.daily_remaining, 20.0);
        assert_eq!(stats.monthly_remaining, 100.0);
        assert!(stats.has_budget(15.0));
        assert!(!stats.has_budget(25.0)); // Exceeds daily remaining
    }
}
