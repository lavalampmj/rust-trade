//! Import command - import data from third-party files
//!
//! Supports multiple import formats:
//! - `csv`: Generic CSV format with configurable columns
//! - `kraken`: Kraken Time & Sales CSV format (from ZIP archive)

use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::{Args, Subcommand};
use rust_decimal::Decimal;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::str::FromStr;
use tracing::{error, info, warn};

use crate::config::Settings;
use crate::storage::MarketDataRepository;
use trading_common::data::types::{TickData, TradeSide};

pub mod kraken;

/// Import commands
#[derive(Subcommand)]
pub enum ImportCommands {
    /// Import generic CSV file
    Csv(CsvImportArgs),
    /// Import Kraken Time & Sales data
    Kraken(kraken::KrakenImportArgs),
}

/// Execute import subcommand
pub async fn execute(cmd: ImportCommands) -> Result<()> {
    match cmd {
        ImportCommands::Csv(args) => execute_csv(args).await,
        ImportCommands::Kraken(args) => kraken::execute(args).await,
    }
}

/// Arguments for the generic CSV import command
#[derive(Args)]
pub struct CsvImportArgs {
    /// Input file path
    #[arg(long, short)]
    pub input: PathBuf,

    /// Exchange for the data
    #[arg(long, short)]
    pub exchange: String,

    /// Symbol (if not in data)
    #[arg(long, short)]
    pub symbol: Option<String>,

    /// Skip header row
    #[arg(long)]
    pub skip_header: bool,

    /// Batch size for inserts
    #[arg(long, default_value = "1000")]
    pub batch_size: usize,

    /// Dry run (parse but don't insert)
    #[arg(long)]
    pub dry_run: bool,
}

/// Execute the generic CSV import command
async fn execute_csv(args: CsvImportArgs) -> Result<()> {
    info!("CSV Import request:");
    info!("  Input: {:?}", args.input);
    info!("  Exchange: {}", args.exchange);
    if let Some(ref symbol) = args.symbol {
        info!("  Symbol: {}", symbol);
    }

    if !args.input.exists() {
        error!("Input file not found: {:?}", args.input);
        return Ok(());
    }

    // Load settings and connect to database
    let settings = Settings::default_settings();

    let repository = if !args.dry_run {
        info!("Connecting to database...");
        Some(MarketDataRepository::from_settings(&settings.database).await?)
    } else {
        None
    };

    // Import CSV
    import_csv_file(&args, repository.as_ref()).await?;

    info!("Import completed");
    Ok(())
}

/// Import from CSV file
async fn import_csv_file(args: &CsvImportArgs, repository: Option<&MarketDataRepository>) -> Result<()> {
    let file = File::open(&args.input)?;
    let reader = BufReader::new(file);

    let mut ticks = Vec::new();
    let mut total_lines = 0;
    let mut total_imported = 0;
    let mut errors = 0;
    let mut sequence = 0i64;

    for (i, line) in reader.lines().enumerate() {
        let line = line?;
        total_lines += 1;

        // Skip header
        if args.skip_header && i == 0 {
            continue;
        }

        // Parse line
        match parse_csv_line(&line, &args.exchange, args.symbol.as_deref(), sequence) {
            Ok(tick) => {
                ticks.push(tick);
                sequence += 1;

                // Batch insert
                if ticks.len() >= args.batch_size {
                    if let Some(repo) = repository {
                        let inserted = repo.batch_insert_ticks(&ticks).await?;
                        total_imported += inserted;
                    } else {
                        total_imported += ticks.len();
                    }
                    ticks.clear();

                    if total_imported % 10000 == 0 {
                        info!("Processed {} records...", total_imported);
                    }
                }
            }
            Err(e) => {
                if errors < 10 {
                    warn!("Line {}: {}", i + 1, e);
                }
                errors += 1;
            }
        }
    }

    // Insert remaining
    if !ticks.is_empty() {
        if let Some(repo) = repository {
            let inserted = repo.batch_insert_ticks(&ticks).await?;
            total_imported += inserted;
        } else {
            total_imported += ticks.len();
        }
    }

    info!("Import summary:");
    info!("  Total lines: {}", total_lines);
    info!("  Imported: {}", total_imported);
    info!("  Errors: {}", errors);

    Ok(())
}

/// Parse a CSV line into a TickData
///
/// Expected format: timestamp,symbol,price,size,side
/// or with symbol from args: timestamp,price,size,side
fn parse_csv_line(
    line: &str,
    exchange: &str,
    default_symbol: Option<&str>,
    sequence: i64,
) -> Result<TickData> {
    let fields: Vec<&str> = line.split(',').map(|s| s.trim()).collect();

    let (timestamp_str, symbol, price_str, size_str, side_str) = if default_symbol.is_some() {
        // Format: timestamp,price,size,side
        if fields.len() < 4 {
            anyhow::bail!("Expected at least 4 fields");
        }
        (
            fields[0],
            default_symbol.unwrap(),
            fields[1],
            fields[2],
            fields[3],
        )
    } else {
        // Format: timestamp,symbol,price,size,side
        if fields.len() < 5 {
            anyhow::bail!("Expected at least 5 fields");
        }
        (fields[0], fields[1], fields[2], fields[3], fields[4])
    };

    // Parse timestamp (support multiple formats)
    let timestamp = parse_timestamp(timestamp_str)?;

    // Parse price and size
    let price = Decimal::from_str(price_str)?;
    let quantity = Decimal::from_str(size_str)?;

    // Parse side
    let side = TradeSide::from_db_str(side_str)
        .ok_or_else(|| anyhow::anyhow!("Invalid side: {}", side_str))?;

    Ok(TickData::with_details(
        timestamp,
        timestamp,
        symbol.to_string(),
        exchange.to_string(),
        price,
        quantity,
        side,
        "import".to_string(),
        format!("import_{}", sequence),
        false,
        sequence,
    ))
}

/// Parse timestamp from various formats
fn parse_timestamp(s: &str) -> Result<DateTime<Utc>> {
    // Try ISO 8601
    if let Ok(ts) = DateTime::parse_from_rfc3339(s) {
        return Ok(ts.with_timezone(&Utc));
    }

    // Try common formats
    let formats = [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
    ];

    for format in &formats {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, format) {
            return Ok(naive.and_utc());
        }
    }

    // Try Unix timestamp (seconds or milliseconds)
    if let Ok(ts) = s.parse::<i64>() {
        if ts > 1_000_000_000_000 {
            // Milliseconds
            return Ok(DateTime::from_timestamp_millis(ts)
                .ok_or_else(|| anyhow::anyhow!("Invalid timestamp"))?);
        } else {
            // Seconds
            return Ok(DateTime::from_timestamp(ts, 0)
                .ok_or_else(|| anyhow::anyhow!("Invalid timestamp"))?);
        }
    }

    anyhow::bail!("Could not parse timestamp: {}", s)
}
