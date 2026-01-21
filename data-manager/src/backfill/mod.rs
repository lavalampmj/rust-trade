//! Backfill module for data-manager
//!
//! Implements automatic data backfill with user configuration and cost controls.
//!
//! ## Features
//!
//! - **Cost estimation**: Estimate costs before fetching data
//! - **Spend tracking**: Daily and monthly spend limits
//! - **Auto-approval**: Small requests can be auto-approved
//! - **Gap detection**: Detect missing data periods
//!
//! ## Usage
//!
//! ```ignore
//! use data_manager::backfill::{BackfillExecutor, DisabledBackfillService};
//! use trading_common::data::backfill::BackfillService;
//!
//! // Create executor (requires provider and repository)
//! let executor = BackfillExecutor::new(config, provider, repository, job_queue);
//!
//! // Estimate cost before fetching
//! let estimate = executor.estimate_cost(&["ES"], start, end).await?;
//!
//! // Request backfill (may require confirmation)
//! let job_id = executor.request_backfill(request).await?;
//! ```

mod executor;
mod spend_tracker;

pub use executor::{BackfillExecutor, DisabledBackfillService};
pub use spend_tracker::{SpendRecord, SpendTracker};

// Re-export types from trading-common for convenience
pub use trading_common::data::backfill::{
    BackfillError, BackfillRequest, BackfillResult, BackfillService, BackfillServiceResult,
    BackfillSource, BackfillStatus, CostEstimate, DataGap, SpendStats,
};
pub use trading_common::data::backfill_config::{
    BackfillConfig, BackfillMode, CostLimitsConfig, OnDemandConfig, ProviderConfig, ScheduledConfig,
};
pub use trading_common::data::gap_detection::{GapDetectionConfig, GapDetector, TradingHours};
