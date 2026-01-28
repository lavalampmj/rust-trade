//! Kraken Time & Sales CSV import command
//!
//! Imports historical trade data from Kraken's CSV export format.

use anyhow::Result;
use chrono::NaiveDate;
use clap::Args;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use tracing::{debug, error, info, warn};
use zip::ZipArchive;

use crate::config::Settings;
use crate::provider::kraken::csv_parser::{self, CsvTradeIterator};
use crate::storage::MarketDataRepository;
use trading_common::data::types::TickData;

/// Arguments for the Kraken import command
#[derive(Args, Debug)]
pub struct KrakenImportArgs {
    /// Input CSV file (single file import)
    #[arg(long, conflicts_with = "zip")]
    pub input: Option<PathBuf>,

    /// Input ZIP archive containing CSV files
    #[arg(long, conflicts_with = "input")]
    pub zip: Option<PathBuf>,

    /// Symbols to import (canonical format, e.g., BTCUSD,ETHUSD)
    /// If not specified with --all, imports nothing
    #[arg(long, value_delimiter = ',')]
    pub symbols: Option<Vec<String>>,

    /// Import all symbols from ZIP archive
    #[arg(long)]
    pub all: bool,

    /// Batch size for database inserts
    #[arg(long, default_value = "10000")]
    pub batch_size: usize,

    /// Start date filter (YYYY-MM-DD), inclusive
    #[arg(long)]
    pub start: Option<NaiveDate>,

    /// End date filter (YYYY-MM-DD), inclusive
    #[arg(long)]
    pub end: Option<NaiveDate>,

    /// Infer trade side using Lee-Ready tick rule (default: true)
    #[arg(long, default_value = "true")]
    pub infer_side: bool,

    /// Dry run - parse and validate without inserting
    #[arg(long)]
    pub dry_run: bool,

    /// Show progress bar
    #[arg(long, default_value = "true")]
    pub progress: bool,

    /// List available symbols in ZIP without importing
    #[arg(long)]
    pub list: bool,
}

/// Execute the Kraken import command
pub async fn execute(args: KrakenImportArgs) -> Result<()> {
    info!("Kraken CSV Import");
    info!("  Infer side: {}", args.infer_side);
    info!("  Batch size: {}", args.batch_size);
    if let Some(ref start) = args.start {
        info!("  Start date: {}", start);
    }
    if let Some(ref end) = args.end {
        info!("  End date: {}", end);
    }
    if args.dry_run {
        info!("  Mode: DRY RUN");
    }

    // Validate args
    if args.input.is_none() && args.zip.is_none() {
        error!("Must specify either --input or --zip");
        return Ok(());
    }

    // Handle ZIP archive
    if let Some(ref zip_path) = args.zip {
        if !zip_path.exists() {
            error!("ZIP file not found: {:?}", zip_path);
            return Ok(());
        }
        return import_from_zip(&args, zip_path).await;
    }

    // Handle single CSV file
    if let Some(ref input_path) = args.input {
        if !input_path.exists() {
            error!("Input file not found: {:?}", input_path);
            return Ok(());
        }
        return import_single_file(&args, input_path).await;
    }

    Ok(())
}

/// Import from a ZIP archive
async fn import_from_zip(args: &KrakenImportArgs, zip_path: &PathBuf) -> Result<()> {
    info!("Opening ZIP archive: {:?}", zip_path);

    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;

    // Collect all CSV files and their canonical symbols
    let mut csv_files: Vec<(String, String)> = Vec::new(); // (entry_name, canonical_symbol)

    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        let name = entry.name().to_string();

        if name.ends_with(".csv") || name.ends_with(".CSV") {
            // Extract filename from path (e.g., "TimeAndSales_Combined/XBTUSD.csv" -> "XBTUSD.csv")
            let filename = name.rsplit('/').next().unwrap_or(&name);

            match csv_parser::parse_filename(filename) {
                Ok((canonical, _, _)) => {
                    csv_files.push((name, canonical));
                }
                Err(e) => {
                    debug!("Skipping {}: {}", name, e);
                }
            }
        }
    }

    info!("Found {} CSV files in archive", csv_files.len());

    // List mode - just show available symbols
    if args.list {
        let mut symbols: Vec<_> = csv_files.iter().map(|(_, s)| s.as_str()).collect();
        symbols.sort();
        symbols.dedup();
        println!("\nAvailable symbols ({}):", symbols.len());
        for symbol in symbols {
            println!("  {}", symbol);
        }
        return Ok(());
    }

    // Determine which symbols to import
    let symbols_to_import: HashSet<String> = if args.all {
        csv_files.iter().map(|(_, s)| s.clone()).collect()
    } else if let Some(ref symbols) = args.symbols {
        symbols.iter().map(|s| s.to_uppercase()).collect()
    } else {
        error!("Must specify --symbols or --all");
        return Ok(());
    };

    if symbols_to_import.is_empty() {
        error!("No symbols to import");
        return Ok(());
    }

    info!("Importing {} symbols", symbols_to_import.len());

    // Filter files to import
    let files_to_import: Vec<_> = csv_files
        .into_iter()
        .filter(|(_, symbol)| symbols_to_import.contains(symbol))
        .collect();

    if files_to_import.is_empty() {
        error!("No matching files found in archive");
        return Ok(());
    }

    // Connect to database (unless dry run)
    let repository = if !args.dry_run {
        let settings = Settings::default_settings();
        info!("Connecting to database...");
        Some(MarketDataRepository::from_settings(&settings.database).await?)
    } else {
        None
    };

    // Import each file
    let mut total_imported = 0usize;
    let mut total_errors = 0usize;

    // Re-open the archive for reading (ZipArchive doesn't allow concurrent access)
    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;

    for (entry_name, canonical_symbol) in &files_to_import {
        info!("Importing {} -> {}", entry_name, canonical_symbol);

        match import_zip_entry(
            &mut archive,
            entry_name,
            canonical_symbol,
            args,
            repository.as_ref(),
        )
        .await
        {
            Ok((imported, errors)) => {
                total_imported += imported;
                total_errors += errors;
                info!("  {} trades imported, {} errors", imported, errors);
            }
            Err(e) => {
                error!("  Failed: {}", e);
                total_errors += 1;
            }
        }
    }

    info!("\nImport complete:");
    info!("  Total trades imported: {}", total_imported);
    info!("  Total errors: {}", total_errors);

    Ok(())
}

/// Import a single entry from a ZIP archive
async fn import_zip_entry<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    entry_name: &str,
    canonical_symbol: &str,
    args: &KrakenImportArgs,
    repository: Option<&MarketDataRepository>,
) -> Result<(usize, usize)> {
    let entry = archive.by_name(entry_name)?;
    let size = entry.size();

    // Create progress bar if enabled
    let progress = if args.progress {
        let pb = ProgressBar::new(size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
                .progress_chars("#>-"),
        );
        Some(pb)
    } else {
        None
    };

    let reader = BufReader::new(entry);
    import_from_reader(reader, canonical_symbol, args, repository, progress).await
}

/// Import from a single CSV file
async fn import_single_file(args: &KrakenImportArgs, input_path: &PathBuf) -> Result<()> {
    // Parse symbol from filename
    let filename = input_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let (canonical_symbol, _, _) = csv_parser::parse_filename(filename)?;
    info!("Symbol: {} (from filename)", canonical_symbol);

    // Connect to database (unless dry run)
    let repository = if !args.dry_run {
        let settings = Settings::default_settings();
        info!("Connecting to database...");
        Some(MarketDataRepository::from_settings(&settings.database).await?)
    } else {
        None
    };

    let file = File::open(input_path)?;
    let size = file.metadata()?.len();

    // Create progress bar if enabled
    let progress = if args.progress {
        let pb = ProgressBar::new(size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
                .progress_chars("#>-"),
        );
        Some(pb)
    } else {
        None
    };

    let reader = BufReader::new(file);
    let (imported, errors) =
        import_from_reader(reader, &canonical_symbol, args, repository.as_ref(), progress).await?;

    info!("\nImport complete:");
    info!("  Trades imported: {}", imported);
    info!("  Errors: {}", errors);

    Ok(())
}

/// Import trades from a reader
async fn import_from_reader<R: Read>(
    reader: R,
    canonical_symbol: &str,
    args: &KrakenImportArgs,
    repository: Option<&MarketDataRepository>,
    progress: Option<ProgressBar>,
) -> Result<(usize, usize)> {
    let mut iter = CsvTradeIterator::new(reader, args.infer_side);
    let mut batch: Vec<TickData> = Vec::with_capacity(args.batch_size);
    let mut total_imported = 0usize;
    let mut total_errors = 0usize;
    let mut sequence = 0i64;

    // Date filters
    let start_ts = args.start.map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc());
    let end_ts = args.end.map(|d| d.and_hms_opt(23, 59, 59).unwrap().and_utc());

    loop {
        match iter.next() {
            Some(Ok(trade)) => {
                // Apply date filter
                if let Some(start) = start_ts {
                    if trade.timestamp < start {
                        continue;
                    }
                }
                if let Some(end) = end_ts {
                    if trade.timestamp > end {
                        continue;
                    }
                }

                // Convert to TickData
                let tick = TickData::with_details(
                    trade.timestamp,
                    trade.timestamp, // ts_recv = ts_event for historical
                    canonical_symbol.to_string(),
                    "KRAKEN".to_string(),
                    trade.price,
                    trade.volume,
                    trade.side, // Already trading_common::TradeSide
                    "kraken_csv".to_string(),
                    format!("{}_{}", trade.timestamp.timestamp(), sequence),
                    false,
                    sequence,
                );

                batch.push(tick);
                sequence += 1;

                // Flush batch
                if batch.len() >= args.batch_size {
                    if let Some(repo) = repository {
                        let inserted = repo.batch_insert_ticks(&batch).await?;
                        total_imported += inserted;
                    } else {
                        total_imported += batch.len();
                    }
                    batch.clear();

                    if let Some(ref pb) = progress {
                        pb.set_position(iter.line_number() as u64 * 30); // Approximate bytes
                    }
                }
            }
            Some(Err(e)) => {
                total_errors += 1;
                if total_errors <= 10 {
                    warn!("Line {}: {}", iter.line_number(), e);
                }
            }
            None => break,
        }
    }

    // Flush remaining
    if !batch.is_empty() {
        if let Some(repo) = repository {
            let inserted = repo.batch_insert_ticks(&batch).await?;
            total_imported += inserted;
        } else {
            total_imported += batch.len();
        }
    }

    if let Some(pb) = progress {
        pb.finish_with_message("done");
    }

    total_errors += iter.error_count();

    Ok((total_imported, total_errors))
}

