//! Database management commands

use anyhow::Result;
use clap::{Args, Subcommand};
use tracing::info;

use crate::config::Settings;
use crate::storage::{MarketDataRepository, TimescaleOperations};

/// Database subcommands
#[derive(Subcommand)]
pub enum DbCommands {
    /// Run database migrations
    Migrate(MigrateArgs),
    /// Show database statistics
    Stats(StatsArgs),
    /// Compress old data
    Compress(CompressArgs),
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

/// Execute database commands
pub async fn execute(cmd: DbCommands) -> Result<()> {
    match cmd {
        DbCommands::Migrate(args) => execute_migrate(args).await,
        DbCommands::Stats(args) => execute_stats(args).await,
        DbCommands::Compress(args) => execute_compress(args).await,
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
