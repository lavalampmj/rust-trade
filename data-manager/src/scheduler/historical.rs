//! Historical data fetch job scheduler

use chrono::{DateTime, Duration, Utc};
use std::collections::VecDeque;
use std::sync::Arc;
use parking_lot::Mutex;
use tracing::debug;
use uuid::Uuid;

use crate::provider::HistoricalRequest;
use crate::symbol::SymbolSpec;
use crate::storage::JobStatus;

/// Historical fetch job
#[derive(Debug, Clone)]
pub struct HistoricalFetchJob {
    /// Unique job ID
    pub id: Uuid,
    /// Symbols to fetch
    pub symbols: Vec<SymbolSpec>,
    /// Start time
    pub start: DateTime<Utc>,
    /// End time
    pub end: DateTime<Utc>,
    /// Provider to use
    pub provider: String,
    /// Dataset (provider-specific)
    pub dataset: Option<String>,
    /// Job priority (higher = sooner)
    pub priority: i32,
    /// Job status
    pub status: JobStatus,
    /// Progress (0-100)
    pub progress: u8,
    /// Records processed
    pub records_processed: u64,
    /// Error message if failed
    pub error: Option<String>,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Started timestamp
    pub started_at: Option<DateTime<Utc>>,
    /// Completed timestamp
    pub completed_at: Option<DateTime<Utc>>,
}

impl HistoricalFetchJob {
    /// Create a new historical fetch job
    pub fn new(
        symbols: Vec<SymbolSpec>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        provider: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            symbols,
            start,
            end,
            provider,
            dataset: None,
            priority: 0,
            status: JobStatus::Pending,
            progress: 0,
            records_processed: 0,
            error: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        }
    }

    /// Set the dataset
    pub fn with_dataset(mut self, dataset: String) -> Self {
        self.dataset = Some(dataset);
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Convert to historical request
    pub fn to_request(&self) -> HistoricalRequest {
        let mut request = HistoricalRequest::trades(
            self.symbols.clone(),
            self.start,
            self.end,
        );
        if let Some(ref dataset) = self.dataset {
            request = request.with_dataset(dataset.clone());
        }
        request
    }

    /// Mark job as started
    pub fn mark_started(&mut self) {
        self.status = JobStatus::Running;
        self.started_at = Some(Utc::now());
    }

    /// Update progress
    pub fn update_progress(&mut self, progress: u8, records: u64) {
        self.progress = progress.min(100);
        self.records_processed = records;
    }

    /// Mark job as completed
    pub fn mark_completed(&mut self) {
        self.status = JobStatus::Completed;
        self.progress = 100;
        self.completed_at = Some(Utc::now());
    }

    /// Mark job as failed
    pub fn mark_failed(&mut self, error: String) {
        self.status = JobStatus::Failed;
        self.error = Some(error);
        self.completed_at = Some(Utc::now());
    }

    /// Get job duration
    pub fn duration(&self) -> Option<Duration> {
        match (self.started_at, self.completed_at) {
            (Some(start), Some(end)) => Some(end - start),
            (Some(start), None) => Some(Utc::now() - start),
            _ => None,
        }
    }
}

/// Job queue for historical fetch jobs
pub struct JobQueue {
    /// Pending jobs (priority queue)
    pending: Arc<Mutex<VecDeque<HistoricalFetchJob>>>,
    /// Currently running jobs
    running: Arc<Mutex<Vec<HistoricalFetchJob>>>,
    /// Completed jobs (recent history)
    completed: Arc<Mutex<VecDeque<HistoricalFetchJob>>>,
    /// Maximum concurrent jobs
    max_concurrent: usize,
    /// Maximum completed history
    max_history: usize,
}

impl JobQueue {
    /// Create a new job queue
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            pending: Arc::new(Mutex::new(VecDeque::new())),
            running: Arc::new(Mutex::new(Vec::new())),
            completed: Arc::new(Mutex::new(VecDeque::new())),
            max_concurrent,
            max_history: 100,
        }
    }

    /// Submit a new job
    pub fn submit(&self, job: HistoricalFetchJob) -> Uuid {
        let id = job.id;
        let mut pending = self.pending.lock();

        // Insert by priority (higher priority first)
        let pos = pending
            .iter()
            .position(|j| j.priority < job.priority)
            .unwrap_or(pending.len());
        pending.insert(pos, job);

        debug!("Submitted job {} (pending: {})", id, pending.len());
        id
    }

    /// Get the next job to run
    pub fn next(&self) -> Option<HistoricalFetchJob> {
        let running_count = self.running.lock().len();
        if running_count >= self.max_concurrent {
            return None;
        }

        let mut pending = self.pending.lock();
        if let Some(mut job) = pending.pop_front() {
            job.mark_started();
            self.running.lock().push(job.clone());
            Some(job)
        } else {
            None
        }
    }

    /// Update a running job
    pub fn update(&self, id: Uuid, progress: u8, records: u64) {
        let mut running = self.running.lock();
        if let Some(job) = running.iter_mut().find(|j| j.id == id) {
            job.update_progress(progress, records);
        }
    }

    /// Complete a job
    pub fn complete(&self, id: Uuid, success: bool, error: Option<String>) {
        let mut running = self.running.lock();
        if let Some(pos) = running.iter().position(|j| j.id == id) {
            let mut job = running.remove(pos);

            if success {
                job.mark_completed();
            } else {
                job.mark_failed(error.unwrap_or_else(|| "Unknown error".to_string()));
            }

            let mut completed = self.completed.lock();
            completed.push_front(job);

            // Trim history
            while completed.len() > self.max_history {
                completed.pop_back();
            }
        }
    }

    /// Cancel a pending job
    pub fn cancel(&self, id: Uuid) -> bool {
        let mut pending = self.pending.lock();
        if let Some(pos) = pending.iter().position(|j| j.id == id) {
            pending.remove(pos);
            true
        } else {
            false
        }
    }

    /// Get job by ID
    pub fn get(&self, id: Uuid) -> Option<HistoricalFetchJob> {
        // Check pending
        if let Some(job) = self.pending.lock().iter().find(|j| j.id == id) {
            return Some(job.clone());
        }

        // Check running
        if let Some(job) = self.running.lock().iter().find(|j| j.id == id) {
            return Some(job.clone());
        }

        // Check completed
        if let Some(job) = self.completed.lock().iter().find(|j| j.id == id) {
            return Some(job.clone());
        }

        None
    }

    /// Get queue statistics
    pub fn stats(&self) -> QueueStats {
        QueueStats {
            pending: self.pending.lock().len(),
            running: self.running.lock().len(),
            completed: self.completed.lock().len(),
            max_concurrent: self.max_concurrent,
        }
    }

    /// List all pending jobs
    pub fn list_pending(&self) -> Vec<HistoricalFetchJob> {
        self.pending.lock().iter().cloned().collect()
    }

    /// List all running jobs
    pub fn list_running(&self) -> Vec<HistoricalFetchJob> {
        self.running.lock().clone()
    }

    /// List recent completed jobs
    pub fn list_completed(&self, limit: usize) -> Vec<HistoricalFetchJob> {
        self.completed.lock().iter().take(limit).cloned().collect()
    }
}

impl Default for JobQueue {
    fn default() -> Self {
        Self::new(4) // 4 concurrent jobs by default
    }
}

/// Queue statistics
#[derive(Debug, Clone)]
pub struct QueueStats {
    pub pending: usize,
    pub running: usize,
    pub completed: usize,
    pub max_concurrent: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_lifecycle() {
        let mut job = HistoricalFetchJob::new(
            vec![SymbolSpec::new("ES", "CME")],
            Utc::now() - Duration::days(1),
            Utc::now(),
            "databento".to_string(),
        );

        assert_eq!(job.status, JobStatus::Pending);

        job.mark_started();
        assert_eq!(job.status, JobStatus::Running);
        assert!(job.started_at.is_some());

        job.update_progress(50, 5000);
        assert_eq!(job.progress, 50);
        assert_eq!(job.records_processed, 5000);

        job.mark_completed();
        assert_eq!(job.status, JobStatus::Completed);
        assert_eq!(job.progress, 100);
        assert!(job.completed_at.is_some());
    }

    #[test]
    fn test_job_queue() {
        let queue = JobQueue::new(2);

        // Submit jobs
        let job1 = HistoricalFetchJob::new(
            vec![SymbolSpec::new("ES", "CME")],
            Utc::now(),
            Utc::now(),
            "test".to_string(),
        );
        let job2 = HistoricalFetchJob::new(
            vec![SymbolSpec::new("NQ", "CME")],
            Utc::now(),
            Utc::now(),
            "test".to_string(),
        )
        .with_priority(10); // Higher priority

        let _id1 = queue.submit(job1);
        let id2 = queue.submit(job2);

        // Higher priority job should come first
        let next = queue.next().unwrap();
        assert_eq!(next.id, id2);

        let stats = queue.stats();
        assert_eq!(stats.pending, 1);
        assert_eq!(stats.running, 1);
    }

    #[test]
    fn test_max_concurrent() {
        let queue = JobQueue::new(1);

        let job1 = HistoricalFetchJob::new(
            vec![SymbolSpec::new("ES", "CME")],
            Utc::now(),
            Utc::now(),
            "test".to_string(),
        );
        let job2 = HistoricalFetchJob::new(
            vec![SymbolSpec::new("NQ", "CME")],
            Utc::now(),
            Utc::now(),
            "test".to_string(),
        );

        queue.submit(job1);
        queue.submit(job2);

        // First job starts
        let first = queue.next();
        assert!(first.is_some());

        // Second job can't start (max concurrent = 1)
        let second = queue.next();
        assert!(second.is_none());
    }
}
