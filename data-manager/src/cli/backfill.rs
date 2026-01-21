//! Backfill CLI commands
//!
//! Provides commands for managing data backfill:
//! - `estimate` - Estimate cost before fetching
//! - `fetch` - Fetch data with cost confirmation
//! - `fill-gaps` - Detect and fill data gaps
//! - `status` - Show job status and spend tracking

use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use clap::{Args, Subcommand};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::backfill::{BackfillExecutor, BackfillRequest, BackfillService, BackfillSource};
use crate::config::Settings;
use crate::provider::databento::DatabentoClient;
use crate::provider::DataProvider;
use crate::scheduler::JobQueue;
use crate::storage::MarketDataRepository;

/// Backfill subcommands
#[derive(Subcommand)]
pub enum BackfillCommands {
    /// Estimate cost for a backfill request
    Estimate(EstimateArgs),
    /// Fetch data with cost confirmation
    Fetch(FetchBackfillArgs),
    /// Detect and fill data gaps
    FillGaps(FillGapsArgs),
    /// Show backfill status and spend tracking
    Status(StatusArgs),
    /// Show cost report
    CostReport(CostReportArgs),
}

/// Arguments for estimate command
#[derive(Args)]
pub struct EstimateArgs {
    /// Symbols to estimate (comma-separated)
    #[arg(long, short, value_delimiter = ',')]
    pub symbols: Vec<String>,

    /// Start date (YYYY-MM-DD)
    #[arg(long)]
    pub start: String,

    /// End date (YYYY-MM-DD)
    #[arg(long)]
    pub end: String,

    /// Dataset to use (default: GLBX.MDP3)
    #[arg(long)]
    pub dataset: Option<String>,
}

/// Arguments for fetch command
#[derive(Args)]
pub struct FetchBackfillArgs {
    /// Symbols to fetch (comma-separated)
    #[arg(long, short, value_delimiter = ',')]
    pub symbols: Vec<String>,

    /// Start date (YYYY-MM-DD)
    #[arg(long)]
    pub start: String,

    /// End date (YYYY-MM-DD)
    #[arg(long)]
    pub end: String,

    /// Dataset to use (default: GLBX.MDP3)
    #[arg(long)]
    pub dataset: Option<String>,

    /// Priority (higher = more urgent)
    #[arg(long, default_value = "5")]
    pub priority: i32,

    /// Confirm and proceed with the fetch
    #[arg(long)]
    pub confirm: bool,

    /// Dry run - show what would be done without executing
    #[arg(long)]
    pub dry_run: bool,
}

/// Arguments for fill-gaps command
#[derive(Args)]
pub struct FillGapsArgs {
    /// Symbols to check for gaps (comma-separated)
    #[arg(long, short, value_delimiter = ',')]
    pub symbols: Vec<String>,

    /// Lookback period in days
    #[arg(long, default_value = "7")]
    pub lookback_days: u32,

    /// Minimum gap size in minutes to fill
    #[arg(long, default_value = "5")]
    pub min_gap_minutes: u32,

    /// Confirm and proceed with filling gaps
    #[arg(long)]
    pub confirm: bool,

    /// Dry run - show gaps without filling
    #[arg(long)]
    pub dry_run: bool,
}

/// Arguments for status command
#[derive(Args)]
pub struct StatusArgs {
    /// Show detailed job information
    #[arg(long)]
    pub detailed: bool,

    /// Filter by status (pending, running, completed, failed)
    #[arg(long)]
    pub status: Option<String>,
}

/// Arguments for cost-report command
#[derive(Args)]
pub struct CostReportArgs {
    /// Show daily breakdown
    #[arg(long)]
    pub daily: bool,

    /// Number of days to show
    #[arg(long, default_value = "7")]
    pub days: u32,
}

/// Execute backfill commands
pub async fn execute(cmd: BackfillCommands) -> Result<()> {
    match cmd {
        BackfillCommands::Estimate(args) => execute_estimate(args).await,
        BackfillCommands::Fetch(args) => execute_fetch(args).await,
        BackfillCommands::FillGaps(args) => execute_fill_gaps(args).await,
        BackfillCommands::Status(args) => execute_status(args).await,
        BackfillCommands::CostReport(args) => execute_cost_report(args).await,
    }
}

/// Execute estimate command
async fn execute_estimate(args: EstimateArgs) -> Result<()> {
    // Parse dates
    let start_date = NaiveDate::parse_from_str(&args.start, "%Y-%m-%d")?;
    let end_date = NaiveDate::parse_from_str(&args.end, "%Y-%m-%d")?;

    let start: DateTime<Utc> = start_date.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end: DateTime<Utc> = end_date.and_hms_opt(23, 59, 59).unwrap().and_utc();

    // Load settings and create executor
    let settings = Settings::load().unwrap_or_else(|_| Settings::default_settings());

    if !settings.backfill.enabled {
        error!("Backfill is disabled in configuration.");
        error!("To enable, set backfill.enabled = true in config/development.toml");
        return Ok(());
    }

    // Create provider
    let api_key = std::env::var("DATABENTO_API_KEY")
        .map_err(|_| anyhow::anyhow!("DATABENTO_API_KEY environment variable not set"))?;
    let mut provider = DatabentoClient::from_api_key(api_key);
    provider.connect().await?;

    // Create repository and job queue
    let repository = Arc::new(MarketDataRepository::from_settings(&settings.database).await?);
    let job_queue = Arc::new(JobQueue::default());

    // Create executor
    let executor = BackfillExecutor::new(
        settings.backfill.clone(),
        Arc::new(provider),
        repository,
        job_queue,
    );

    // Estimate cost
    let estimate = executor.estimate_cost(&args.symbols, start, end).await?;

    // Display results
    println!();
    println!("=== Backfill Cost Estimate ===");
    println!();
    println!("Symbols:          {:?}", args.symbols);
    println!("Time Range:       {} to {} ({} days)", args.start, args.end, (end - start).num_days());
    println!("Estimated Cost:   ${:.2}", estimate.estimated_cost_usd);
    println!("Estimated Records: ~{}", format_number(estimate.estimated_records));
    println!();

    if estimate.exceeds_limits {
        println!("⚠️  This request EXCEEDS configured limits:");
        for warning in &estimate.warnings {
            println!("   - {}", warning);
        }
        println!();
    } else if !estimate.can_auto_approve {
        println!("⚠️  This request requires confirmation.");
        println!("   Run with --confirm to proceed.");
        println!();
    } else {
        println!("✓ This request can be auto-approved (below threshold).");
        println!();
    }

    println!("Current Spend:");
    println!("   Daily:   ${:.2} / ${:.2}", estimate.current_daily_spend, settings.backfill.cost_limits.max_daily_spend);
    println!("   Monthly: ${:.2} / ${:.2}", estimate.current_monthly_spend, settings.backfill.cost_limits.max_monthly_spend);
    println!();

    Ok(())
}

/// Execute fetch command
async fn execute_fetch(args: FetchBackfillArgs) -> Result<()> {
    // Parse dates
    let start_date = NaiveDate::parse_from_str(&args.start, "%Y-%m-%d")?;
    let end_date = NaiveDate::parse_from_str(&args.end, "%Y-%m-%d")?;

    let start: DateTime<Utc> = start_date.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end: DateTime<Utc> = end_date.and_hms_opt(23, 59, 59).unwrap().and_utc();

    // Load settings
    let settings = Settings::load().unwrap_or_else(|_| Settings::default_settings());

    if !settings.backfill.enabled {
        error!("Backfill is disabled in configuration.");
        error!("To enable, set backfill.enabled = true in config/development.toml");
        return Ok(());
    }

    // Create provider
    let api_key = std::env::var("DATABENTO_API_KEY")
        .map_err(|_| anyhow::anyhow!("DATABENTO_API_KEY environment variable not set"))?;
    let mut provider = DatabentoClient::from_api_key(api_key);
    provider.connect().await?;

    // Create repository and job queue
    let repository = Arc::new(MarketDataRepository::from_settings(&settings.database).await?);
    let job_queue = Arc::new(JobQueue::default());

    // Create executor
    let executor = BackfillExecutor::new(
        settings.backfill.clone(),
        Arc::new(provider),
        repository,
        job_queue,
    );

    // First estimate cost
    let estimate = executor.estimate_cost(&args.symbols, start, end).await?;

    println!();
    println!("=== Backfill Cost Estimate ===");
    println!();
    println!("Symbols:          {:?}", args.symbols);
    println!("Time Range:       {} to {} ({} days)", args.start, args.end, (end - start).num_days());
    println!("Estimated Cost:   ${:.2}", estimate.estimated_cost_usd);
    println!("Estimated Records: ~{}", format_number(estimate.estimated_records));
    println!();

    if args.dry_run {
        println!("Dry run mode - not executing.");
        return Ok(());
    }

    if estimate.exceeds_limits {
        error!("Request exceeds configured limits:");
        for warning in &estimate.warnings {
            error!("   - {}", warning);
        }
        return Ok(());
    }

    if !args.confirm && !estimate.can_auto_approve {
        warn!("This request requires confirmation.");
        warn!("Run with --confirm to proceed, or --dry-run to preview.");
        return Ok(());
    }

    // Create and submit request
    info!("Submitting backfill request...");
    let mut request = BackfillRequest::with_symbols(
        args.symbols.clone(),
        start,
        end,
        BackfillSource::Manual,
    )
    .with_priority(args.priority);

    if args.confirm {
        request = request.with_auto_approve();
    }

    if let Some(dataset) = args.dataset {
        request = request.with_dataset(dataset);
    }

    match executor.request_backfill(request).await {
        Ok(job_id) => {
            println!("✓ Backfill job submitted: {}", job_id);
            println!("  Use 'data-manager backfill status' to check progress.");
        }
        Err(e) => {
            error!("Failed to submit backfill: {}", e);
        }
    }

    Ok(())
}

/// Execute fill-gaps command
async fn execute_fill_gaps(args: FillGapsArgs) -> Result<()> {
    let end = Utc::now();
    let start = end - chrono::Duration::days(args.lookback_days as i64);

    // Load settings
    let settings = Settings::load().unwrap_or_else(|_| Settings::default_settings());

    if !settings.backfill.enabled {
        error!("Backfill is disabled in configuration.");
        error!("To enable, set backfill.enabled = true in config/development.toml");
        return Ok(());
    }

    // Create provider
    let api_key = std::env::var("DATABENTO_API_KEY")
        .map_err(|_| anyhow::anyhow!("DATABENTO_API_KEY environment variable not set"))?;
    let mut provider = DatabentoClient::from_api_key(api_key);
    provider.connect().await?;

    // Create repository and job queue
    let repository = Arc::new(MarketDataRepository::from_settings(&settings.database).await?);
    let job_queue = Arc::new(JobQueue::default());

    // Create executor
    let executor = BackfillExecutor::new(
        settings.backfill.clone(),
        Arc::new(provider),
        repository,
        job_queue,
    );

    println!("Detecting data gaps for {} symbols...", args.symbols.len());
    println!("Lookback: {} days", args.lookback_days);
    println!("Minimum gap: {} minutes", args.min_gap_minutes);
    println!();

    let mut total_gaps = Vec::new();

    for symbol in &args.symbols {
        let gaps = executor.detect_gaps(symbol, start, end).await?;

        // Filter by minimum size
        let filtered_gaps: Vec<_> = gaps
            .into_iter()
            .filter(|g| g.duration_minutes >= args.min_gap_minutes as i64)
            .collect();

        if !filtered_gaps.is_empty() {
            println!("{}: {} gaps detected", symbol, filtered_gaps.len());
            for gap in &filtered_gaps {
                println!(
                    "  - {} to {} ({} min, ~{} records)",
                    gap.start.format("%Y-%m-%d %H:%M"),
                    gap.end.format("%Y-%m-%d %H:%M"),
                    gap.duration_minutes,
                    gap.estimated_records.unwrap_or(0)
                );
            }
            total_gaps.extend(filtered_gaps);
        } else {
            println!("{}: No gaps detected", symbol);
        }
    }

    println!();
    println!("Total gaps: {}", total_gaps.len());

    if total_gaps.is_empty() {
        println!("No gaps to fill.");
        return Ok(());
    }

    // Estimate total cost
    let total_minutes: i64 = total_gaps.iter().map(|g| g.duration_minutes).sum();
    let total_estimated_records: u64 = total_gaps
        .iter()
        .filter_map(|g| g.estimated_records)
        .sum();

    println!("Total gap duration: {} minutes", total_minutes);
    println!("Total estimated records: ~{}", format_number(total_estimated_records));

    if args.dry_run {
        println!();
        println!("Dry run mode - not filling gaps.");
        return Ok(());
    }

    if !args.confirm {
        warn!("Run with --confirm to fill gaps, or --dry-run to preview.");
        return Ok(());
    }

    info!("Filling {} gaps...", total_gaps.len());

    // Submit backfill requests for each gap
    for gap in total_gaps {
        let request = BackfillRequest::new(&gap.symbol, gap.start, gap.end, BackfillSource::Manual)
            .with_auto_approve();

        match executor.request_backfill(request).await {
            Ok(job_id) => {
                println!("✓ Submitted gap fill for {}: {}", gap.symbol, job_id);
            }
            Err(e) => {
                error!("Failed to submit gap fill for {}: {}", gap.symbol, e);
            }
        }
    }

    Ok(())
}

/// Execute status command
async fn execute_status(args: StatusArgs) -> Result<()> {
    let settings = Settings::load().unwrap_or_else(|_| Settings::default_settings());

    println!("=== Backfill Status ===");
    println!();

    if !settings.backfill.enabled {
        println!("Status: DISABLED");
        println!();
        println!("Backfill is disabled in configuration.");
        println!("To enable, set backfill.enabled = true in config/development.toml");
        return Ok(());
    }

    println!("Status: ENABLED");
    println!("Mode: {}", settings.backfill.mode);
    println!();

    println!("Cost Limits:");
    println!("  Per-request max: ${:.2}", settings.backfill.cost_limits.max_cost_per_request);
    println!("  Daily max:       ${:.2}", settings.backfill.cost_limits.max_daily_spend);
    println!("  Monthly max:     ${:.2}", settings.backfill.cost_limits.max_monthly_spend);
    println!("  Auto-approve:    {} (threshold: ${:.2})",
        if settings.backfill.cost_limits.auto_approve_enabled { "enabled" } else { "disabled" },
        settings.backfill.cost_limits.auto_approve_threshold
    );
    println!();

    if args.detailed {
        println!("On-Demand Settings:");
        println!("  Enabled: {}", settings.backfill.on_demand.enabled);
        println!("  Max lookback: {} days", settings.backfill.on_demand.max_lookback_days);
        println!("  Min gap: {} minutes", settings.backfill.on_demand.min_gap_minutes);
        println!("  Cooldown: {} seconds", settings.backfill.on_demand.cooldown_secs);
        println!();

        println!("Provider Settings:");
        println!("  Name: {}", settings.backfill.provider.name);
        println!("  Dataset: {}", settings.backfill.provider.dataset);
        println!("  Max concurrent: {}", settings.backfill.provider.max_concurrent_jobs);
        println!();
    }

    // TODO: Show actual spend and job statistics when connected to executor
    println!("Note: Detailed job and spend tracking requires running executor.");

    Ok(())
}

/// Execute cost-report command
async fn execute_cost_report(_args: CostReportArgs) -> Result<()> {
    let settings = Settings::load().unwrap_or_else(|_| Settings::default_settings());

    println!("=== Backfill Cost Report ===");
    println!();

    if !settings.backfill.enabled {
        println!("Backfill is disabled. No costs recorded.");
        return Ok(());
    }

    println!("Limits:");
    println!("  Daily:   ${:.2}", settings.backfill.cost_limits.max_daily_spend);
    println!("  Monthly: ${:.2}", settings.backfill.cost_limits.max_monthly_spend);
    println!();

    // TODO: Show actual spend records when connected to persistent storage
    println!("Note: Detailed cost history requires persistent spend tracking.");
    println!("Current implementation tracks spend in-memory only.");

    Ok(())
}

/// Format large numbers with commas
fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.insert(0, ',');
        }
        result.insert(0, c);
    }
    result
}
