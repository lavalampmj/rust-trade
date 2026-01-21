//! Backfill executor implementation
//!
//! Implements the BackfillService trait, handling cost estimation,
//! data fetching, and spend tracking.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use trading_common::data::backfill::{
    BackfillError, BackfillRequest as CommonBackfillRequest, BackfillResult as CommonBackfillResult,
    BackfillService, BackfillServiceResult, BackfillSource, BackfillStatus, CostEstimate, DataGap,
    SpendStats,
};
use trading_common::data::backfill_config::BackfillConfig;
use trading_common::data::gap_detection::{GapDetectionConfig, GapDetector};

use crate::provider::{HistoricalDataProvider, HistoricalRequest};
use crate::scheduler::JobQueue;
use crate::storage::MarketDataRepository;
use crate::symbol::SymbolSpec;

use super::spend_tracker::SpendTracker;

/// Internal job state for backfill operations
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used for future job tracking functionality
struct BackfillJob {
    /// Original request
    request: CommonBackfillRequest,
    /// Current status
    status: BackfillStatus,
    /// Cost estimate (once calculated)
    cost_estimate: Option<CostEstimate>,
    /// Scheduler job ID (once submitted to queue)
    scheduler_job_id: Option<Uuid>,
    /// Result (once completed)
    result: Option<CommonBackfillResult>,
    /// Created at
    created_at: DateTime<Utc>,
    /// Updated at
    updated_at: DateTime<Utc>,
}

impl BackfillJob {
    fn new(request: CommonBackfillRequest) -> Self {
        let now = Utc::now();
        Self {
            request,
            status: BackfillStatus::Pending,
            cost_estimate: None,
            scheduler_job_id: None,
            result: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Backfill executor - implements BackfillService
pub struct BackfillExecutor<P: HistoricalDataProvider> {
    /// Configuration
    config: BackfillConfig,
    /// Provider for fetching data
    provider: Arc<P>,
    /// Repository for storing data
    repository: Arc<MarketDataRepository>,
    /// Job queue for scheduling fetches
    job_queue: Arc<JobQueue>,
    /// Spend tracker
    spend_tracker: Arc<SpendTracker>,
    /// Active jobs
    jobs: RwLock<HashMap<Uuid, BackfillJob>>,
    /// Cooldown tracker (symbol -> last request time)
    cooldowns: RwLock<HashMap<String, DateTime<Utc>>>,
}

impl<P: HistoricalDataProvider + 'static> BackfillExecutor<P> {
    /// Create a new backfill executor
    pub fn new(
        config: BackfillConfig,
        provider: Arc<P>,
        repository: Arc<MarketDataRepository>,
        job_queue: Arc<JobQueue>,
    ) -> Self {
        let spend_tracker = Arc::new(SpendTracker::new(config.cost_limits.clone()));

        Self {
            config,
            provider,
            repository,
            job_queue,
            spend_tracker,
            jobs: RwLock::new(HashMap::new()),
            cooldowns: RwLock::new(HashMap::new()),
        }
    }

    /// Get the spend tracker
    pub fn spend_tracker(&self) -> Arc<SpendTracker> {
        self.spend_tracker.clone()
    }

    /// Check if symbol is in cooldown
    fn is_in_cooldown(&self, symbol: &str) -> bool {
        if let Some(last_request) = self.cooldowns.read().get(symbol) {
            let cooldown_duration = chrono::Duration::seconds(self.config.on_demand.cooldown_secs as i64);
            Utc::now() < *last_request + cooldown_duration
        } else {
            false
        }
    }

    /// Set cooldown for symbol
    fn set_cooldown(&self, symbol: &str) {
        self.cooldowns.write().insert(symbol.to_string(), Utc::now());
    }

    /// Estimate cost for Databento data
    ///
    /// This is a simplified estimation based on time range and expected data density.
    /// In production, you would call Databento's metadata API for accurate estimates.
    fn estimate_databento_cost(
        &self,
        symbols: &[String],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> (f64, u64) {
        // Rough estimation based on Databento pricing
        // Actual pricing depends on dataset, data type, and volume
        let duration_hours = (end - start).num_hours() as f64;
        let symbol_count = symbols.len() as f64;

        // Estimate ~$0.01 per symbol-hour for tick data (rough approximation)
        // This should be replaced with actual Databento cost API
        let estimated_cost = duration_hours * symbol_count * 0.01;

        // Estimate ~1000 ticks per symbol per minute during market hours
        // This varies significantly by symbol and time
        let estimated_minutes = (end - start).num_minutes() as u64;
        let estimated_records = estimated_minutes * symbols.len() as u64 * 100; // Conservative

        (estimated_cost, estimated_records)
    }

    /// Execute a backfill job (internal)
    async fn execute_job(&self, job_id: Uuid) -> BackfillServiceResult<CommonBackfillResult> {
        // Get job info
        let job = {
            let jobs = self.jobs.read();
            jobs.get(&job_id).cloned()
        };

        let job = match job {
            Some(j) => j,
            None => return Err(BackfillError::NotFound(job_id)),
        };

        // Update status to running
        {
            let mut jobs = self.jobs.write();
            if let Some(j) = jobs.get_mut(&job_id) {
                j.status = BackfillStatus::Running;
                j.updated_at = Utc::now();
            }
        }

        let start_time = std::time::Instant::now();

        // Build historical request
        let symbols: Vec<SymbolSpec> = job
            .request
            .symbols
            .iter()
            .map(|s| {
                // Parse symbol - assume format "SYMBOL@EXCHANGE" or just "SYMBOL"
                let parts: Vec<&str> = s.split('@').collect();
                if parts.len() == 2 {
                    SymbolSpec::new(parts[0], parts[1])
                } else {
                    // Default to CME for futures
                    SymbolSpec::new(s, "CME")
                }
            })
            .collect();

        let dataset = job
            .request
            .dataset
            .clone()
            .unwrap_or_else(|| self.config.provider.dataset.clone());

        let request = HistoricalRequest::trades(symbols.clone(), job.request.start, job.request.end)
            .with_dataset(dataset);

        // Fetch data from provider
        info!(
            "Starting backfill for {} symbols from {} to {}",
            symbols.len(),
            job.request.start,
            job.request.end
        );

        let fetch_result = self.provider.fetch_ticks(&request).await;

        match fetch_result {
            Ok(tick_iter) => {
                let mut records_fetched = 0u64;
                let mut records_inserted = 0u64;
                let mut batch = Vec::with_capacity(1000);

                // Process ticks in batches
                for tick_result in tick_iter {
                    match tick_result {
                        Ok(tick) => {
                            records_fetched += 1;
                            batch.push(tick);

                            // Insert in batches
                            if batch.len() >= 1000 {
                                match self.repository.batch_insert_ticks(&batch).await {
                                    Ok(inserted) => records_inserted += inserted as u64,
                                    Err(e) => {
                                        warn!("Batch insert error: {}", e);
                                    }
                                }
                                batch.clear();
                            }
                        }
                        Err(e) => {
                            warn!("Error fetching tick: {}", e);
                        }
                    }
                }

                // Insert remaining batch
                if !batch.is_empty() {
                    match self.repository.batch_insert_ticks(&batch).await {
                        Ok(inserted) => records_inserted += inserted as u64,
                        Err(e) => {
                            warn!("Final batch insert error: {}", e);
                        }
                    }
                }

                let duration_secs = start_time.elapsed().as_secs_f64();
                let actual_cost = job
                    .cost_estimate
                    .as_ref()
                    .map(|e| e.estimated_cost_usd)
                    .unwrap_or(0.0);

                // Record spend
                self.spend_tracker.record_with_job(
                    actual_cost,
                    &format!("Backfill {} symbols", symbols.len()),
                    &job_id.to_string(),
                );

                // Update cooldowns
                for symbol in &job.request.symbols {
                    self.set_cooldown(symbol);
                }

                let result = CommonBackfillResult {
                    job_id,
                    status: BackfillStatus::Completed,
                    records_fetched,
                    records_inserted,
                    actual_cost_usd: actual_cost,
                    duration_secs,
                    error: None,
                    completed_at: Utc::now(),
                };

                info!(
                    "Backfill completed: {} records fetched, {} inserted in {:.2}s",
                    records_fetched, records_inserted, duration_secs
                );

                // Update job state
                {
                    let mut jobs = self.jobs.write();
                    if let Some(j) = jobs.get_mut(&job_id) {
                        j.status = BackfillStatus::Completed;
                        j.result = Some(result.clone());
                        j.updated_at = Utc::now();
                    }
                }

                Ok(result)
            }
            Err(e) => {
                let error_msg = format!("Provider error: {}", e);
                error!("Backfill failed: {}", error_msg);

                let result = CommonBackfillResult {
                    job_id,
                    status: BackfillStatus::Failed,
                    records_fetched: 0,
                    records_inserted: 0,
                    actual_cost_usd: 0.0,
                    duration_secs: start_time.elapsed().as_secs_f64(),
                    error: Some(error_msg.clone()),
                    completed_at: Utc::now(),
                };

                // Update job state
                {
                    let mut jobs = self.jobs.write();
                    if let Some(j) = jobs.get_mut(&job_id) {
                        j.status = BackfillStatus::Failed;
                        j.result = Some(result.clone());
                        j.updated_at = Utc::now();
                    }
                }

                Err(BackfillError::Provider(error_msg))
            }
        }
    }
}

#[async_trait]
impl<P: HistoricalDataProvider + Send + Sync + 'static> BackfillService for BackfillExecutor<P> {
    fn is_enabled(&self) -> bool {
        self.config.is_enabled()
    }

    async fn detect_gaps(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> BackfillServiceResult<Vec<DataGap>> {
        if !self.is_enabled() {
            return Err(BackfillError::Disabled);
        }

        // Parse symbol to get exchange
        let parts: Vec<&str> = symbol.split('@').collect();
        let (sym, exchange) = if parts.len() == 2 {
            (parts[0], parts[1])
        } else {
            (symbol, "CME") // Default
        };

        // Fetch existing ticks
        let existing_ticks = self
            .repository
            .get_ticks(sym, exchange, start, end, None)
            .await
            .map_err(|e| BackfillError::Database(e.to_string()))?;

        // Convert to trading-common TickData format for gap detection
        let ticks: Vec<trading_common::data::types::TickData> = existing_ticks
            .iter()
            .map(|t| {
                trading_common::data::types::TickData::with_details(
                    t.ts_event,
                    t.ts_recv,
                    t.symbol.clone(),
                    t.exchange.clone(),
                    t.price,
                    t.size,
                    match t.side {
                        crate::schema::TradeSide::Buy => trading_common::data::types::TradeSide::Buy,
                        crate::schema::TradeSide::Sell => trading_common::data::types::TradeSide::Sell,
                    },
                    t.provider.clone(),
                    t.provider_trade_id.clone().unwrap_or_else(|| format!("{}_{}", t.symbol, t.ts_event.timestamp_nanos_opt().unwrap_or(0))),
                    t.is_buyer_maker.unwrap_or(false),
                    t.sequence,
                )
            })
            .collect();

        // Detect gaps
        let config = GapDetectionConfig {
            min_gap_minutes: self.config.on_demand.min_gap_minutes as i64,
            expected_ticks_per_minute: 100.0,
            trading_hours: None, // Use symbol-specific trading hours if available
        };
        let detector = GapDetector::new(config);
        let gaps = detector.detect_gaps(symbol, &ticks, start, end);

        debug!("Detected {} gaps for {}", gaps.len(), symbol);
        Ok(gaps)
    }

    async fn estimate_cost(
        &self,
        symbols: &[String],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> BackfillServiceResult<CostEstimate> {
        if !self.is_enabled() {
            return Err(BackfillError::Disabled);
        }

        let (estimated_cost, estimated_records) = self.estimate_databento_cost(symbols, start, end);

        let mut estimate = CostEstimate::new(estimated_cost, estimated_records);

        // Check against limits
        if let Some(reason) = self.spend_tracker.would_exceed_limits(estimated_cost) {
            estimate = estimate.with_exceeds_limits(reason);
        }

        // Check auto-approve
        let (can_auto, reason) = self.spend_tracker.can_auto_approve(estimated_cost);
        estimate = estimate.with_auto_approve(can_auto, reason);

        // Add current spend
        estimate = estimate.with_current_spend(
            self.spend_tracker.daily_spend(),
            self.spend_tracker.monthly_spend(),
        );

        // Add warnings
        for warning in self.spend_tracker.get_warnings() {
            estimate = estimate.with_warning(warning);
        }

        Ok(estimate)
    }

    async fn request_backfill(
        &self,
        request: CommonBackfillRequest,
    ) -> BackfillServiceResult<Uuid> {
        if !self.is_enabled() {
            return Err(BackfillError::Disabled);
        }

        // Check mode allows this source
        match request.source {
            BackfillSource::OnDemand => {
                if !self.config.is_on_demand_enabled() {
                    return Err(BackfillError::Configuration(
                        "On-demand backfill is not enabled".to_string(),
                    ));
                }
            }
            BackfillSource::Scheduled => {
                if !self.config.is_scheduled_enabled() {
                    return Err(BackfillError::Configuration(
                        "Scheduled backfill is not enabled".to_string(),
                    ));
                }
            }
            BackfillSource::Manual => {
                if !self.config.is_manual_allowed() {
                    return Err(BackfillError::Configuration(
                        "Manual backfill is not allowed".to_string(),
                    ));
                }
            }
        }

        // Check cooldowns for on-demand requests
        if request.source == BackfillSource::OnDemand {
            for symbol in &request.symbols {
                if self.is_in_cooldown(symbol) {
                    return Err(BackfillError::Duplicate);
                }
            }
        }

        // Estimate cost
        let cost_estimate = self
            .estimate_cost(&request.symbols, request.start, request.end)
            .await?;

        // Check if exceeds limits
        if cost_estimate.exceeds_limits {
            return Err(BackfillError::CostLimitExceeded(
                cost_estimate.warnings.join("; "),
            ));
        }

        // Check if requires confirmation
        if !request.skip_confirmation && !cost_estimate.can_auto_approve {
            let job_id = request.id;

            // Store job in awaiting confirmation state
            let mut job = BackfillJob::new(request);
            job.status = BackfillStatus::AwaitingConfirmation;
            job.cost_estimate = Some(cost_estimate.clone());

            self.jobs.write().insert(job_id, job);

            return Err(BackfillError::RequiresConfirmation(cost_estimate));
        }

        // Auto-approved or confirmation skipped - execute immediately
        let job_id = request.id;
        let mut job = BackfillJob::new(request);
        job.status = BackfillStatus::Pending;
        job.cost_estimate = Some(cost_estimate);

        self.jobs.write().insert(job_id, job);

        // Execute the job
        // Note: In production, this would be queued to the job scheduler
        // For simplicity, we execute synchronously here
        tokio::spawn({
            let executor = BackfillExecutor {
                config: self.config.clone(),
                provider: self.provider.clone(),
                repository: self.repository.clone(),
                job_queue: self.job_queue.clone(),
                spend_tracker: self.spend_tracker.clone(),
                jobs: RwLock::new(self.jobs.read().clone()),
                cooldowns: RwLock::new(self.cooldowns.read().clone()),
            };

            async move {
                let _ = executor.execute_job(job_id).await;
            }
        });

        Ok(job_id)
    }

    async fn confirm_request(&self, job_id: Uuid) -> BackfillServiceResult<()> {
        let job = {
            let jobs = self.jobs.read();
            jobs.get(&job_id).cloned()
        };

        let job = match job {
            Some(j) => j,
            None => return Err(BackfillError::NotFound(job_id)),
        };

        if job.status != BackfillStatus::AwaitingConfirmation {
            return Err(BackfillError::Internal(format!(
                "Job {} is not awaiting confirmation (status: {})",
                job_id, job.status
            )));
        }

        // Update status and execute
        {
            let mut jobs = self.jobs.write();
            if let Some(j) = jobs.get_mut(&job_id) {
                j.status = BackfillStatus::Pending;
                j.updated_at = Utc::now();
            }
        }

        // Execute the job
        tokio::spawn({
            let executor = BackfillExecutor {
                config: self.config.clone(),
                provider: self.provider.clone(),
                repository: self.repository.clone(),
                job_queue: self.job_queue.clone(),
                spend_tracker: self.spend_tracker.clone(),
                jobs: RwLock::new(self.jobs.read().clone()),
                cooldowns: RwLock::new(self.cooldowns.read().clone()),
            };

            async move {
                let _ = executor.execute_job(job_id).await;
            }
        });

        Ok(())
    }

    async fn cancel_request(&self, job_id: Uuid) -> BackfillServiceResult<()> {
        let mut jobs = self.jobs.write();

        if let Some(job) = jobs.get_mut(&job_id) {
            match job.status {
                BackfillStatus::Pending | BackfillStatus::AwaitingConfirmation => {
                    job.status = BackfillStatus::Cancelled;
                    job.updated_at = Utc::now();
                    info!("Cancelled backfill job {}", job_id);
                    Ok(())
                }
                BackfillStatus::Running => {
                    // Can't cancel running jobs easily
                    Err(BackfillError::Internal(
                        "Cannot cancel running job".to_string(),
                    ))
                }
                _ => {
                    Err(BackfillError::Internal(format!(
                        "Job {} is already {} and cannot be cancelled",
                        job_id, job.status
                    )))
                }
            }
        } else {
            Err(BackfillError::NotFound(job_id))
        }
    }

    async fn get_job_status(&self, job_id: Uuid) -> BackfillServiceResult<BackfillStatus> {
        let jobs = self.jobs.read();
        jobs.get(&job_id)
            .map(|j| j.status)
            .ok_or(BackfillError::NotFound(job_id))
    }

    async fn get_job_result(
        &self,
        job_id: Uuid,
    ) -> BackfillServiceResult<Option<CommonBackfillResult>> {
        let jobs = self.jobs.read();
        Ok(jobs.get(&job_id).and_then(|j| j.result.clone()))
    }

    async fn get_spend_stats(&self) -> BackfillServiceResult<SpendStats> {
        Ok(self.spend_tracker.stats())
    }
}

/// No-op backfill service for when backfill is disabled
pub struct DisabledBackfillService;

#[async_trait]
impl BackfillService for DisabledBackfillService {
    fn is_enabled(&self) -> bool {
        false
    }

    async fn detect_gaps(
        &self,
        _symbol: &str,
        _start: DateTime<Utc>,
        _end: DateTime<Utc>,
    ) -> BackfillServiceResult<Vec<DataGap>> {
        Err(BackfillError::Disabled)
    }

    async fn estimate_cost(
        &self,
        _symbols: &[String],
        _start: DateTime<Utc>,
        _end: DateTime<Utc>,
    ) -> BackfillServiceResult<CostEstimate> {
        Err(BackfillError::Disabled)
    }

    async fn request_backfill(
        &self,
        _request: CommonBackfillRequest,
    ) -> BackfillServiceResult<Uuid> {
        Err(BackfillError::Disabled)
    }

    async fn confirm_request(&self, _job_id: Uuid) -> BackfillServiceResult<()> {
        Err(BackfillError::Disabled)
    }

    async fn cancel_request(&self, _job_id: Uuid) -> BackfillServiceResult<()> {
        Err(BackfillError::Disabled)
    }

    async fn get_job_status(&self, _job_id: Uuid) -> BackfillServiceResult<BackfillStatus> {
        Err(BackfillError::Disabled)
    }

    async fn get_job_result(
        &self,
        _job_id: Uuid,
    ) -> BackfillServiceResult<Option<CommonBackfillResult>> {
        Err(BackfillError::Disabled)
    }

    async fn get_spend_stats(&self) -> BackfillServiceResult<SpendStats> {
        Err(BackfillError::Disabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disabled_service() {
        let service = DisabledBackfillService;
        assert!(!service.is_enabled());
    }
}
