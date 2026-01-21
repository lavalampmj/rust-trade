//! Spend tracking for backfill operations
//!
//! Tracks daily and monthly spending to enforce cost limits.

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info};

use trading_common::data::backfill::SpendStats;
use trading_common::data::backfill_config::CostLimitsConfig;

/// Spend record for a single request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendRecord {
    /// Timestamp of the spend
    pub timestamp: DateTime<Utc>,
    /// Amount spent (USD)
    pub amount: f64,
    /// Job ID associated with this spend
    pub job_id: Option<String>,
    /// Description
    pub description: String,
}

impl SpendRecord {
    /// Create a new spend record
    pub fn new(amount: f64, description: String) -> Self {
        Self {
            timestamp: Utc::now(),
            amount,
            job_id: None,
            description,
        }
    }

    /// Set the job ID
    pub fn with_job_id(mut self, job_id: String) -> Self {
        self.job_id = Some(job_id);
        self
    }
}

/// Spend tracker for monitoring and limiting costs
pub struct SpendTracker {
    /// Cost limits configuration
    limits: CostLimitsConfig,
    /// Daily spend records by date (YYYY-MM-DD)
    daily_records: RwLock<HashMap<String, Vec<SpendRecord>>>,
    /// Monthly spend records by month (YYYY-MM)
    monthly_totals: RwLock<HashMap<String, f64>>,
}

impl SpendTracker {
    /// Create a new spend tracker with the given limits
    pub fn new(limits: CostLimitsConfig) -> Self {
        Self {
            limits,
            daily_records: RwLock::new(HashMap::new()),
            monthly_totals: RwLock::new(HashMap::new()),
        }
    }

    /// Get the date key for today (YYYY-MM-DD)
    fn today_key() -> String {
        Utc::now().format("%Y-%m-%d").to_string()
    }

    /// Get the month key for current month (YYYY-MM)
    fn month_key() -> String {
        Utc::now().format("%Y-%m").to_string()
    }

    /// Get current daily spend
    pub fn daily_spend(&self) -> f64 {
        let key = Self::today_key();
        self.daily_records
            .read()
            .get(&key)
            .map(|records| records.iter().map(|r| r.amount).sum())
            .unwrap_or(0.0)
    }

    /// Get current monthly spend
    pub fn monthly_spend(&self) -> f64 {
        let key = Self::month_key();
        *self.monthly_totals.read().get(&key).unwrap_or(&0.0)
    }

    /// Get spend statistics
    pub fn stats(&self) -> SpendStats {
        SpendStats::new(
            self.daily_spend(),
            self.limits.max_daily_spend,
            self.monthly_spend(),
            self.limits.max_monthly_spend,
        )
    }

    /// Check if spending an amount would exceed limits
    pub fn would_exceed_limits(&self, amount: f64) -> Option<String> {
        let daily = self.daily_spend();
        let monthly = self.monthly_spend();

        // Check per-request limit
        if amount > self.limits.max_cost_per_request {
            return Some(format!(
                "Request cost ${:.2} exceeds per-request limit ${:.2}",
                amount, self.limits.max_cost_per_request
            ));
        }

        // Check daily limit
        if daily + amount > self.limits.max_daily_spend {
            return Some(format!(
                "Would exceed daily limit: ${:.2} + ${:.2} > ${:.2}",
                daily, amount, self.limits.max_daily_spend
            ));
        }

        // Check monthly limit
        if monthly + amount > self.limits.max_monthly_spend {
            return Some(format!(
                "Would exceed monthly limit: ${:.2} + ${:.2} > ${:.2}",
                monthly, amount, self.limits.max_monthly_spend
            ));
        }

        None
    }

    /// Check if an amount can be auto-approved
    pub fn can_auto_approve(&self, amount: f64) -> (bool, Option<String>) {
        // Check if auto-approve is enabled
        if !self.limits.auto_approve_enabled {
            return (false, Some("Auto-approval is disabled".to_string()));
        }

        // Check if amount is below threshold
        if amount > self.limits.auto_approve_threshold {
            return (
                false,
                Some(format!(
                    "Amount ${:.2} exceeds auto-approve threshold ${:.2}",
                    amount, self.limits.auto_approve_threshold
                )),
            );
        }

        // Check if it would exceed limits
        if let Some(reason) = self.would_exceed_limits(amount) {
            return (false, Some(reason));
        }

        (true, None)
    }

    /// Check if an amount requires confirmation
    pub fn requires_confirmation(&self, amount: f64) -> bool {
        !self.can_auto_approve(amount).0 && amount > self.limits.confirmation_threshold
    }

    /// Record a spend
    pub fn record_spend(&self, record: SpendRecord) {
        let daily_key = record.timestamp.format("%Y-%m-%d").to_string();
        let monthly_key = record.timestamp.format("%Y-%m").to_string();

        info!(
            "Recording spend: ${:.2} for {}",
            record.amount, record.description
        );

        // Add to daily records
        {
            let mut daily = self.daily_records.write();
            daily.entry(daily_key).or_insert_with(Vec::new).push(record.clone());
        }

        // Update monthly total
        {
            let mut monthly = self.monthly_totals.write();
            *monthly.entry(monthly_key).or_insert(0.0) += record.amount;
        }

        debug!(
            "Updated spend totals: daily=${:.2}, monthly=${:.2}",
            self.daily_spend(),
            self.monthly_spend()
        );
    }

    /// Record a spend by amount and description
    pub fn record(&self, amount: f64, description: &str) {
        self.record_spend(SpendRecord::new(amount, description.to_string()));
    }

    /// Record a spend with job ID
    pub fn record_with_job(&self, amount: f64, description: &str, job_id: &str) {
        let record = SpendRecord::new(amount, description.to_string())
            .with_job_id(job_id.to_string());
        self.record_spend(record);
    }

    /// Get today's spend records
    pub fn today_records(&self) -> Vec<SpendRecord> {
        let key = Self::today_key();
        self.daily_records
            .read()
            .get(&key)
            .cloned()
            .unwrap_or_default()
    }

    /// Get spend records for a specific date
    pub fn records_for_date(&self, date: &str) -> Vec<SpendRecord> {
        self.daily_records
            .read()
            .get(date)
            .cloned()
            .unwrap_or_default()
    }

    /// Clear old records (keeps last 30 days)
    pub fn cleanup_old_records(&self) {
        let cutoff = Utc::now() - chrono::Duration::days(30);
        let cutoff_date = cutoff.format("%Y-%m-%d").to_string();
        let cutoff_month = cutoff.format("%Y-%m").to_string();

        // Clean daily records
        {
            let mut daily = self.daily_records.write();
            daily.retain(|date, _| date >= &cutoff_date);
        }

        // Clean monthly totals (keep last 3 months)
        {
            let mut monthly = self.monthly_totals.write();
            monthly.retain(|month, _| month >= &cutoff_month);
        }

        debug!("Cleaned up old spend records");
    }

    /// Get remaining daily budget
    pub fn daily_remaining(&self) -> f64 {
        (self.limits.max_daily_spend - self.daily_spend()).max(0.0)
    }

    /// Get remaining monthly budget
    pub fn monthly_remaining(&self) -> f64 {
        (self.limits.max_monthly_spend - self.monthly_spend()).max(0.0)
    }

    /// Check if there's sufficient budget for an amount
    pub fn has_budget(&self, amount: f64) -> bool {
        self.would_exceed_limits(amount).is_none()
    }

    /// Get warnings for current spend state
    pub fn get_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let daily = self.daily_spend();
        let monthly = self.monthly_spend();

        // Warn if approaching daily limit (>80%)
        let daily_pct = daily / self.limits.max_daily_spend * 100.0;
        if daily_pct >= 80.0 {
            warnings.push(format!(
                "Daily spend at {:.0}% of limit (${:.2} / ${:.2})",
                daily_pct, daily, self.limits.max_daily_spend
            ));
        }

        // Warn if approaching monthly limit (>80%)
        let monthly_pct = monthly / self.limits.max_monthly_spend * 100.0;
        if monthly_pct >= 80.0 {
            warnings.push(format!(
                "Monthly spend at {:.0}% of limit (${:.2} / ${:.2})",
                monthly_pct, monthly, self.limits.max_monthly_spend
            ));
        }

        warnings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_limits() -> CostLimitsConfig {
        CostLimitsConfig {
            max_cost_per_request: 10.0,
            max_daily_spend: 50.0,
            max_monthly_spend: 500.0,
            confirmation_threshold: 5.0,
            auto_approve_threshold: 1.0,
            auto_approve_enabled: true,
        }
    }

    #[test]
    fn test_initial_spend_is_zero() {
        let tracker = SpendTracker::new(test_limits());
        assert_eq!(tracker.daily_spend(), 0.0);
        assert_eq!(tracker.monthly_spend(), 0.0);
    }

    #[test]
    fn test_record_spend() {
        let tracker = SpendTracker::new(test_limits());
        tracker.record(5.0, "Test spend");

        assert_eq!(tracker.daily_spend(), 5.0);
        assert_eq!(tracker.monthly_spend(), 5.0);
    }

    #[test]
    fn test_would_exceed_daily_limit() {
        let tracker = SpendTracker::new(test_limits());
        tracker.record(45.0, "Initial spend");

        assert!(tracker.would_exceed_limits(10.0).is_some());
        assert!(tracker.would_exceed_limits(5.0).is_none());
    }

    #[test]
    fn test_would_exceed_per_request_limit() {
        let tracker = SpendTracker::new(test_limits());
        assert!(tracker.would_exceed_limits(15.0).is_some());
    }

    #[test]
    fn test_auto_approve() {
        let tracker = SpendTracker::new(test_limits());

        let (can_approve, _) = tracker.can_auto_approve(0.5);
        assert!(can_approve);

        let (can_approve, reason) = tracker.can_auto_approve(1.5);
        assert!(!can_approve);
        assert!(reason.is_some());
    }

    #[test]
    fn test_auto_approve_disabled() {
        let mut limits = test_limits();
        limits.auto_approve_enabled = false;
        let tracker = SpendTracker::new(limits);

        let (can_approve, reason) = tracker.can_auto_approve(0.5);
        assert!(!can_approve);
        assert!(reason.unwrap().contains("disabled"));
    }

    #[test]
    fn test_remaining_budget() {
        let tracker = SpendTracker::new(test_limits());
        tracker.record(20.0, "Test");

        assert_eq!(tracker.daily_remaining(), 30.0);
        assert_eq!(tracker.monthly_remaining(), 480.0);
    }

    #[test]
    fn test_stats() {
        let tracker = SpendTracker::new(test_limits());
        tracker.record(10.0, "Test");

        let stats = tracker.stats();
        assert_eq!(stats.daily_spend, 10.0);
        assert_eq!(stats.daily_limit, 50.0);
        assert_eq!(stats.daily_remaining, 40.0);
    }

    #[test]
    fn test_warnings() {
        let tracker = SpendTracker::new(test_limits());
        tracker.record(45.0, "High spend");

        let warnings = tracker.get_warnings();
        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("90%")); // 45/50 = 90%
    }
}
