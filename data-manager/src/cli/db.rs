//! Database management commands

use anyhow::Result;
use clap::{Args, Subcommand};
use tracing::info;

use crate::config::Settings;
use crate::storage::{MarketDataRepository, OhlcQueryHelper, TimescaleOperations};

/// Database subcommands
#[derive(Subcommand)]
pub enum DbCommands {
    /// Run database migrations
    Migrate(MigrateArgs),
    /// Show database statistics
    Stats(StatsArgs),
    /// Compress old data
    Compress(CompressArgs),
    /// Manage OHLC continuous aggregates
    Aggregate(AggregateArgs),
}

/// Arguments for migrate command
#[derive(Args)]
pub struct MigrateArgs {
    /// Enable compression after migration
    #[arg(long)]
    pub enable_compression: bool,
}

/// Arguments for stats command
#[derive(Args)]
pub struct StatsArgs {
    /// Show per-symbol statistics
    #[arg(long, short)]
    pub verbose: bool,
}

/// Arguments for compress command
#[derive(Args)]
pub struct CompressArgs {
    /// Compress chunks older than N days
    #[arg(long, default_value = "7")]
    pub older_than: i32,
}

/// Arguments for aggregate command
#[derive(Args)]
pub struct AggregateArgs {
    #[command(subcommand)]
    pub command: AggregateCommands,
}

/// Aggregate subcommands
#[derive(Subcommand)]
pub enum AggregateCommands {
    /// Create a continuous aggregate
    Create(AggregateCreateArgs),
    /// Refresh aggregate data for a time range
    Refresh(AggregateRefreshArgs),
    /// Show aggregate statistics
    Stats(AggregateStatsArgs),
    /// Drop a continuous aggregate
    Drop(AggregateDropArgs),
}

/// Arguments for aggregate create command
#[derive(Args)]
pub struct AggregateCreateArgs {
    /// Timeframe for the aggregate (1m, 5m, 15m, 1h, 4h, 1d)
    #[arg(long, default_value = "1m")]
    pub timeframe: String,

    /// Enable automatic refresh policy
    #[arg(long)]
    pub with_refresh: bool,

    /// Refresh start offset (e.g., "1 hour")
    #[arg(long, default_value = "1 hour")]
    pub start_offset: String,

    /// Refresh end offset (e.g., "1 minute")
    #[arg(long, default_value = "1 minute")]
    pub end_offset: String,

    /// Refresh schedule interval (e.g., "1 minute")
    #[arg(long, default_value = "1 minute")]
    pub schedule_interval: String,
}

/// Arguments for aggregate refresh command
#[derive(Args)]
pub struct AggregateRefreshArgs {
    /// Aggregate view name (e.g., ohlc_1m)
    #[arg(long)]
    pub view: String,

    /// Start time (ISO 8601 format)
    #[arg(long)]
    pub start: String,

    /// End time (ISO 8601 format)
    #[arg(long)]
    pub end: String,
}

/// Arguments for aggregate stats command
#[derive(Args)]
pub struct AggregateStatsArgs {
    /// Show per-symbol statistics for a specific aggregate
    #[arg(long)]
    pub view: Option<String>,

    /// Symbol to get statistics for
    #[arg(long)]
    pub symbol: Option<String>,

    /// Exchange to get statistics for
    #[arg(long)]
    pub exchange: Option<String>,
}

/// Arguments for aggregate drop command
#[derive(Args)]
pub struct AggregateDropArgs {
    /// Aggregate view name to drop (e.g., ohlc_1m)
    #[arg(long)]
    pub view: String,

    /// Skip confirmation prompt
    #[arg(long)]
    pub force: bool,
}

/// Execute database commands
pub async fn execute(cmd: DbCommands) -> Result<()> {
    match cmd {
        DbCommands::Migrate(args) => execute_migrate(args).await,
        DbCommands::Stats(args) => execute_stats(args).await,
        DbCommands::Compress(args) => execute_compress(args).await,
        DbCommands::Aggregate(args) => execute_aggregate(args).await,
    }
}

async fn execute_migrate(args: MigrateArgs) -> Result<()> {
    let settings = Settings::default_settings();
    let repository = MarketDataRepository::from_settings(&settings.database).await?;
    let timescale = TimescaleOperations::new(repository.pool().clone());

    info!("Running migrations...");
    timescale.run_migrations().await?;

    if args.enable_compression {
        info!("Enabling compression...");
        timescale.enable_compression().await?;
        timescale.add_compression_policy(7).await?;
    }

    info!("Migrations completed");
    Ok(())
}

async fn execute_stats(args: StatsArgs) -> Result<()> {
    let settings = Settings::default_settings();
    let repository = MarketDataRepository::from_settings(&settings.database).await?;

    info!("Fetching database statistics...");

    let stats = repository.get_database_stats().await?;

    info!("Database Statistics:");
    info!("  Total records: {}", stats.total_records);
    info!("  Total symbols: {}", stats.total_symbols);
    info!("  Total size: {}", stats.total_size);
    if let Some(earliest) = stats.earliest_time {
        info!("  Earliest data: {}", earliest);
    }
    if let Some(latest) = stats.latest_time {
        info!("  Latest data: {}", latest);
    }

    if args.verbose {
        info!("\nPer-symbol statistics:");
        let symbols = repository.get_available_symbols().await?;
        for (symbol, exchange) in symbols {
            let sym_stats = repository.get_symbol_stats(&symbol, &exchange).await?;
            info!(
                "  {}@{}: {} records ({:?} - {:?})",
                symbol,
                exchange,
                sym_stats.total_records,
                sym_stats.earliest_time,
                sym_stats.latest_time
            );
        }
    }

    // Compression stats
    let timescale = TimescaleOperations::new(repository.pool().clone());
    match timescale.get_compression_stats().await {
        Ok(comp_stats) => {
            info!("\nCompression Statistics:");
            info!("  Compressed chunks: {}", comp_stats.compressed_chunks);
            info!("  Uncompressed chunks: {}", comp_stats.uncompressed_chunks);
            info!("  Compressed size: {}", comp_stats.compressed_size);
            info!("  Uncompressed size: {}", comp_stats.uncompressed_size);
        }
        Err(e) => {
            info!("\nCompression stats not available: {}", e);
        }
    }

    Ok(())
}

async fn execute_compress(args: CompressArgs) -> Result<()> {
    let settings = Settings::default_settings();
    let repository = MarketDataRepository::from_settings(&settings.database).await?;
    let timescale = TimescaleOperations::new(repository.pool().clone());

    info!("Compressing chunks older than {} days...", args.older_than);

    let compressed = timescale
        .compress_chunks_older_than(args.older_than)
        .await?;

    info!("Compressed {} chunks", compressed);
    Ok(())
}

async fn execute_aggregate(args: AggregateArgs) -> Result<()> {
    match args.command {
        AggregateCommands::Create(create_args) => execute_aggregate_create(create_args).await,
        AggregateCommands::Refresh(refresh_args) => execute_aggregate_refresh(refresh_args).await,
        AggregateCommands::Stats(stats_args) => execute_aggregate_stats(stats_args).await,
        AggregateCommands::Drop(drop_args) => execute_aggregate_drop(drop_args).await,
    }
}

async fn execute_aggregate_create(args: AggregateCreateArgs) -> Result<()> {
    let settings = Settings::default_settings();
    let repository = MarketDataRepository::from_settings(&settings.database).await?;
    let timescale = TimescaleOperations::new(repository.pool().clone());

    let view_name = format!("ohlc_{}", args.timeframe);

    info!("Creating continuous aggregate {}...", view_name);
    timescale.create_ohlc_aggregate(&args.timeframe).await?;

    if args.with_refresh {
        info!("Adding refresh policy...");
        timescale
            .add_aggregate_refresh_policy(
                &view_name,
                &args.start_offset,
                &args.end_offset,
                &args.schedule_interval,
            )
            .await?;
    }

    info!("Continuous aggregate {} created successfully", view_name);
    println!("\nCreated continuous aggregate: {}", view_name);
    println!("  Timeframe: {}", args.timeframe);
    if args.with_refresh {
        println!("  Refresh policy:");
        println!("    Start offset: {}", args.start_offset);
        println!("    End offset: {}", args.end_offset);
        println!("    Schedule: {}", args.schedule_interval);
    } else {
        println!("  Refresh policy: None (use --with-refresh to enable)");
    }
    println!("\nTo populate historical data, run:");
    println!(
        "  cargo run db aggregate refresh --view {} --start \"2024-01-01\" --end \"2024-01-31\"",
        view_name
    );

    Ok(())
}

async fn execute_aggregate_refresh(args: AggregateRefreshArgs) -> Result<()> {
    let settings = Settings::default_settings();
    let repository = MarketDataRepository::from_settings(&settings.database).await?;
    let timescale = TimescaleOperations::new(repository.pool().clone());

    info!(
        "Refreshing {} from {} to {}...",
        args.view, args.start, args.end
    );

    timescale
        .refresh_aggregate(&args.view, &args.start, &args.end)
        .await?;

    info!("Refresh completed");
    println!("Refreshed {} from {} to {}", args.view, args.start, args.end);

    Ok(())
}

async fn execute_aggregate_stats(args: AggregateStatsArgs) -> Result<()> {
    let settings = Settings::default_settings();
    let repository = MarketDataRepository::from_settings(&settings.database).await?;
    let timescale = TimescaleOperations::new(repository.pool().clone());

    // Show all aggregates info
    info!("Fetching continuous aggregate information...");
    let aggregates = timescale.get_aggregate_info().await?;

    if aggregates.is_empty() {
        println!("No continuous aggregates found.");
        println!("\nTo create one, run:");
        println!("  cargo run db aggregate create --timeframe 1m --with-refresh");
        return Ok(());
    }

    println!("\nContinuous Aggregates:");
    println!("{}", "=".repeat(60));

    for agg in &aggregates {
        println!("\n  View: {}", agg.view_name);
        println!("    Owner: {}", agg.view_owner);
        println!("    Chunks: {}", agg.chunk_count);
        if let Some(ref interval) = agg.refresh_interval {
            println!("    Refresh interval: {}", interval);
        } else {
            println!("    Refresh policy: None");
        }
        if let Some(ref offset) = agg.start_offset {
            println!("    Start offset: {}", offset);
        }
        if let Some(ref offset) = agg.end_offset {
            println!("    End offset: {}", offset);
        }
    }

    // If specific view and symbol requested, show per-symbol stats
    if let (Some(view), Some(symbol)) = (&args.view, &args.symbol) {
        let exchange = args.exchange.as_deref().unwrap_or("binance");
        let ohlc_helper = OhlcQueryHelper::new(repository.pool().clone());

        info!(
            "Fetching statistics for {}@{} from {}...",
            symbol, exchange, view
        );

        match ohlc_helper.get_aggregate_stats(view, symbol, exchange).await {
            Ok(stats) => {
                println!("\n{}@{} in {}:", symbol, exchange, view);
                println!("  Bar count: {}", stats.bar_count);
                if let Some(earliest) = stats.earliest_bucket {
                    println!("  Earliest: {}", earliest);
                }
                if let Some(latest) = stats.latest_bucket {
                    println!("  Latest: {}", latest);
                }
                if let Some(volume) = stats.total_volume {
                    println!("  Total volume: {}", volume);
                }
                println!("  Total trades: {}", stats.total_trades);
            }
            Err(e) => {
                println!("  Error fetching stats: {}", e);
            }
        }
    }

    Ok(())
}

async fn execute_aggregate_drop(args: AggregateDropArgs) -> Result<()> {
    if !args.force {
        println!("WARNING: This will permanently delete the continuous aggregate '{}'", args.view);
        println!("To confirm, run with --force flag");
        return Ok(());
    }

    let settings = Settings::default_settings();
    let repository = MarketDataRepository::from_settings(&settings.database).await?;
    let timescale = TimescaleOperations::new(repository.pool().clone());

    info!("Dropping continuous aggregate {}...", args.view);
    timescale.drop_aggregate(&args.view).await?;

    info!("Dropped {}", args.view);
    println!("Dropped continuous aggregate: {}", args.view);

    Ok(())
}
