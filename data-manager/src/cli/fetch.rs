//! Fetch command - fetch historical data on-demand

use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use clap::Args;
use tracing::{error, info};

use crate::config::Settings;
use crate::provider::databento::DatabentoClient;
use crate::provider::{DataProvider, HistoricalDataProvider, HistoricalRequest};
use crate::storage::MarketDataRepository;
use crate::symbol::SymbolSpec;

/// Arguments for the fetch command
#[derive(Args)]
pub struct FetchArgs {
    /// Symbols to fetch (comma-separated)
    #[arg(long, short, value_delimiter = ',')]
    pub symbols: Vec<String>,

    /// Exchange for the symbols
    #[arg(long, short)]
    pub exchange: String,

    /// Data provider to use
    #[arg(long, short, default_value = "databento")]
    pub provider: String,

    /// Start date (YYYY-MM-DD)
    #[arg(long)]
    pub start: String,

    /// End date (YYYY-MM-DD)
    #[arg(long)]
    pub end: String,

    /// Provider-specific dataset
    #[arg(long)]
    pub dataset: Option<String>,

    /// Dry run (don't actually fetch)
    #[arg(long)]
    pub dry_run: bool,
}

/// Execute the fetch command
pub async fn execute(args: FetchArgs) -> Result<()> {
    // Parse dates
    let start_date = NaiveDate::parse_from_str(&args.start, "%Y-%m-%d")?;
    let end_date = NaiveDate::parse_from_str(&args.end, "%Y-%m-%d")?;

    let start: DateTime<Utc> = start_date.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end: DateTime<Utc> = end_date.and_hms_opt(23, 59, 59).unwrap().and_utc();

    // Create symbol specs
    let symbols: Vec<SymbolSpec> = args
        .symbols
        .iter()
        .map(|s| SymbolSpec::new(s, &args.exchange))
        .collect();

    info!("Fetch request:");
    info!("  Symbols: {:?}", args.symbols);
    info!("  Exchange: {}", args.exchange);
    info!("  Provider: {}", args.provider);
    info!("  Date range: {} to {}", args.start, args.end);
    if let Some(ref dataset) = args.dataset {
        info!("  Dataset: {}", dataset);
    }

    if args.dry_run {
        info!("Dry run - not actually fetching data");
        return Ok(());
    }

    // Load settings
    let settings = Settings::default_settings();

    // Check provider
    if args.provider != "databento" {
        error!("Unknown provider: {}. Only 'databento' is currently supported.", args.provider);
        return Ok(());
    }

    // Check for API key
    let api_key = std::env::var("DATABENTO_API_KEY")
        .map_err(|_| anyhow::anyhow!("DATABENTO_API_KEY environment variable not set"))?;

    // Create provider
    let mut provider = DatabentoClient::from_api_key(api_key);
    provider.connect().await?;

    // Create request
    let mut request = HistoricalRequest::trades(symbols, start, end);
    if let Some(dataset) = args.dataset {
        request = request.with_dataset(dataset);
    }

    // Connect to database
    info!("Connecting to database...");
    let repository = MarketDataRepository::from_settings(&settings.database).await?;

    // Fetch data
    info!("Fetching data...");
    let ticks: Vec<_> = provider
        .fetch_ticks(&request)
        .await?
        .filter_map(|r| r.ok())
        .collect();

    info!("Received {} ticks", ticks.len());

    if ticks.is_empty() {
        info!("No data received");
        return Ok(());
    }

    // Insert into database
    info!("Inserting into database...");
    let inserted = repository.batch_insert_ticks(&ticks).await?;
    info!("Inserted {} ticks", inserted);

    info!("Fetch completed successfully");
    Ok(())
}
