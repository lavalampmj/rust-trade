# Kraken Time & Sales CSV Importer Design

## Overview

Import historical Kraken Time & Sales data from ZIP archive into DBT-compatible format for backtesting and analysis.

## Input Format

**Source**: `Kraken_Trading_History.zip` containing `TimeAndSales_Combined/*.csv`

**Filename Convention**: `{BASE}{QUOTE}.csv`
- Examples: `XBTUSD.csv`, `ETHUSD.csv`, `AAVEEUR.csv`, `ETHXBT.csv`
- Last 3-4 chars typically indicate quote currency
- Kraken uses `XBT` for Bitcoin (convert to `BTC` for canonical format)

**CSV Format** (3 columns, no header):
```
timestamp,price,volume
1381095255,122.0,0.1
1381179030,123.61,0.1
```

| Column | Type | Description |
|--------|------|-------------|
| timestamp | i64 | Unix seconds since epoch |
| price | f64 | Trade price in quote currency |
| volume | f64 | Trade volume in base currency |

**Data Characteristics**:
- ~1,120 symbol files
- Data spans from 2013 (BTC) to present
- No trade side information (buy/sell unknown)
- Fractional volumes common (e.g., 0.001 BTC)

## Side Inference (Lee-Ready Tick Rule)

Since Kraken T&S data doesn't include trade side, we infer it using the **Lee-Ready tick rule**:

```
If price > previous_price → Buy (aggressor bought at higher price)
If price < previous_price → Sell (aggressor sold at lower price)
If price == previous_price → Inherit previous direction
```

**Algorithm**:
```rust
pub struct SideInferrer {
    last_price: Option<Decimal>,
    last_side: TradeSide,
}

impl SideInferrer {
    pub fn new() -> Self {
        Self {
            last_price: None,
            last_side: TradeSide::Buy, // Default for first trade
        }
    }

    pub fn infer(&mut self, price: Decimal) -> TradeSide {
        let side = match self.last_price {
            None => TradeSide::Buy, // First trade defaults to Buy
            Some(prev) => {
                if price > prev {
                    TradeSide::Buy
                } else if price < prev {
                    TradeSide::Sell
                } else {
                    self.last_side // Same price, inherit direction
                }
            }
        };

        self.last_price = Some(price);
        self.last_side = side;
        side
    }
}
```

**Notes**:
- This is an approximation - not the actual exchange-determined side
- Widely used in academic research and industry when side isn't provided
- First trade of each session defaults to `Buy`
- The `TradeSide::Unknown` variant is still available for cases where inference is disabled

**CLI flag**:
```bash
# Enable side inference (default)
cargo run import kraken --zip data/Kraken_Trading_History.zip --infer-side

# Disable side inference (store as Unknown)
cargo run import kraken --zip data/Kraken_Trading_History.zip --no-infer-side
```

## Output Format

### Primary: TimescaleDB Storage

Store as `market_ticks` table with full Decimal precision:

```sql
INSERT INTO market_ticks (
    ts_event, ts_recv, symbol, exchange,
    price, size, side, provider, provider_trade_id,
    is_buyer_maker, raw_dbn, sequence
)
```

**Field Mapping**:
| CSV Field | market_ticks Field | Transformation |
|-----------|-------------------|----------------|
| timestamp | ts_event | Unix seconds → DateTime<Utc> |
| timestamp | ts_recv | Same as ts_event (historical) |
| filename | symbol | Extract + canonicalize (XBTUSD → BTCUSD) |
| - | exchange | "KRAKEN" constant |
| price | price | f64 → Decimal |
| volume | size | f64 → Decimal (full precision) |
| price | side | Lee-Ready inference (Buy/Sell) or Unknown |
| - | provider | "kraken_csv" |
| - | provider_trade_id | "{timestamp}_{sequence}" |
| - | is_buyer_maker | NULL (unknown) |
| - | raw_dbn | NULL |
| - | sequence | Auto-increment per file |

### Secondary: DBN File Export (Optional)

Export to DBN binary format for backtesting:

```rust
TradeMsg {
    hd: RecordHeader {
        publisher_id: CUSTOM_PUBLISHER_ID,
        instrument_id: hash(symbol, exchange),
        ts_event: timestamp_nanos,
    },
    price: decimal_to_dbn_price(price),  // i64 with 1e-9 scale
    size: scale_crypto_size(volume),      // u32 with scaling
    side: 'N' as c_char,                  // None/Unknown
    action: 'T' as c_char,                // Trade
    ...
}
```

**Size Scaling Strategy**:
- Scale factor: 1e8 (satoshi-like)
- `size = (volume * 1e8).round() as u32`
- Max representable: 42.94967295 units (sufficient for most trades)
- Store scale factor in metadata file alongside DBN

## Symbol Conversion

Use existing `data-manager/src/provider/kraken/symbol.rs`:

```rust
// Filename parsing
fn parse_kraken_filename(filename: &str) -> Result<(String, String)> {
    // "XBTUSD.csv" → ("XBT", "USD")
    // "ETHEUR.csv" → ("ETH", "EUR")
    // "1INCHUSD.csv" → ("1INCH", "USD")
}

// Canonical conversion
fn to_canonical(base: &str, quote: &str) -> String {
    // XBT → BTC
    // XBTUSD → BTCUSD
    // ETHEUR → ETHEUR
}
```

**Quote Currency Detection**:
Extended list for T&S data:
```rust
const QUOTE_CURRENCIES: &[&str] = &[
    "USD", "EUR", "GBP", "CAD", "JPY", "AUD", "CHF", "AED",  // Fiat
    "USDT", "USDC", "DAI",                                    // Stablecoins
    "XBT", "ETH", "DOT",                                      // Crypto quotes
];
```

## CLI Interface

```bash
# Import single file
cargo run import kraken \
  --input data/TimeAndSales_Combined/XBTUSD.csv \
  --batch-size 10000

# Import from ZIP archive
cargo run import kraken \
  --zip data/Kraken_Trading_History.zip \
  --symbols BTCUSD,ETHUSD,SOLUSD \
  --batch-size 10000

# Import all symbols from ZIP
cargo run import kraken \
  --zip data/Kraken_Trading_History.zip \
  --all \
  --batch-size 10000

# Dry run to check parsing
cargo run import kraken \
  --zip data/Kraken_Trading_History.zip \
  --symbols BTCUSD \
  --dry-run

# Export to DBN file
cargo run import kraken \
  --zip data/Kraken_Trading_History.zip \
  --symbols BTCUSD \
  --output-dbn data/kraken_btcusd.dbn

# Filter by date range
cargo run import kraken \
  --zip data/Kraken_Trading_History.zip \
  --symbols BTCUSD \
  --start 2024-01-01 \
  --end 2024-12-31
```

## Module Structure

```
data-manager/src/
├── cli/
│   ├── import.rs           # Existing generic import
│   └── import_kraken.rs    # NEW: Kraken-specific import
├── provider/
│   └── kraken/
│       ├── symbol.rs       # Existing symbol conversion
│       └── csv_parser.rs   # NEW: CSV parsing logic
```

### New Files

**`data-manager/src/cli/import_kraken.rs`**:
```rust
#[derive(Args)]
pub struct KrakenImportArgs {
    /// Input CSV file
    #[arg(long, conflicts_with = "zip")]
    pub input: Option<PathBuf>,

    /// Input ZIP archive
    #[arg(long, conflicts_with = "input")]
    pub zip: Option<PathBuf>,

    /// Symbols to import (canonical format)
    #[arg(long, value_delimiter = ',')]
    pub symbols: Option<Vec<String>>,

    /// Import all symbols from ZIP
    #[arg(long)]
    pub all: bool,

    /// Batch size for database inserts
    #[arg(long, default_value = "10000")]
    pub batch_size: usize,

    /// Start date filter (YYYY-MM-DD)
    #[arg(long)]
    pub start: Option<NaiveDate>,

    /// End date filter (YYYY-MM-DD)
    #[arg(long)]
    pub end: Option<NaiveDate>,

    /// Output DBN file path
    #[arg(long)]
    pub output_dbn: Option<PathBuf>,

    /// Dry run (parse only, don't insert)
    #[arg(long)]
    pub dry_run: bool,

    /// Skip database insert (only output DBN)
    #[arg(long)]
    pub skip_db: bool,
}
```

**`data-manager/src/provider/kraken/csv_parser.rs`**:
```rust
/// Kraken CSV trade record
pub struct KrakenCsvTrade {
    pub timestamp: i64,
    pub price: Decimal,
    pub volume: Decimal,
}

/// Parse a single CSV line
pub fn parse_line(line: &str) -> Result<KrakenCsvTrade, ParseError> {
    let parts: Vec<&str> = line.split(',').collect();
    if parts.len() != 3 {
        return Err(ParseError::InvalidFormat);
    }
    Ok(KrakenCsvTrade {
        timestamp: parts[0].parse()?,
        price: Decimal::from_str(parts[1])?,
        volume: Decimal::from_str(parts[2])?,
    })
}

/// Parse symbol from filename
pub fn parse_filename(filename: &str) -> Result<CanonicalSymbol, ParseError> {
    // Remove .csv extension
    // Split into base/quote
    // Convert XBT → BTC
    // Return canonical symbol
}

/// Stream trades from CSV file
pub fn stream_csv_file(path: &Path) -> impl Iterator<Item = Result<KrakenCsvTrade>> {
    BufReader::new(File::open(path)?)
        .lines()
        .map(|line| parse_line(&line?))
}

/// Stream trades from ZIP archive entry
pub fn stream_zip_entry(
    archive: &mut ZipArchive<File>,
    entry_name: &str,
) -> impl Iterator<Item = Result<KrakenCsvTrade>> {
    let entry = archive.by_name(entry_name)?;
    BufReader::new(entry)
        .lines()
        .map(|line| parse_line(&line?))
}
```

## Processing Pipeline

```
┌─────────────────┐
│   ZIP Archive   │
│ or CSV File     │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Parse Filename │ → Extract symbol, convert to canonical
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Stream Lines   │ → BufReader for memory efficiency
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Parse Fields   │ → timestamp, price, volume
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Date Filter    │ → Optional start/end filtering
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Batch Collect  │ → Collect batch_size records
└────────┬────────┘
         │
    ┌────┴────┐
    ▼         ▼
┌────────┐ ┌────────┐
│  DB    │ │  DBN   │
│ Insert │ │ Export │
└────────┘ └────────┘
```

## Performance Considerations

1. **Streaming**: Use `BufReader` to avoid loading entire files into memory
2. **Batch Inserts**: Insert in batches of 10,000 rows (configurable)
3. **ZIP Streaming**: Use `zip` crate's streaming API (don't extract to disk)
4. **Parallel Files**: Optionally process multiple CSV files in parallel with `rayon`
5. **Progress Reporting**: Show progress every N records

**Estimated Performance**:
- ~500K lines/second parsing
- ~50K inserts/second to TimescaleDB
- Full archive (~12GB compressed): ~2-4 hours

## Error Handling

1. **Invalid lines**: Log and skip, continue processing
2. **Parse errors**: Collect error count, warn at threshold
3. **Database errors**: Retry with exponential backoff
4. **ZIP corruption**: Fail fast with clear error message

## Validation

1. **Timestamp sanity**: Check within reasonable range (2010-2030)
2. **Price positive**: price > 0
3. **Volume positive**: volume > 0
4. **Symbol format**: Valid after conversion

## Instrument Registry Integration

Register symbols with `InstrumentRegistry` for persistent IDs:

```rust
let registry = InstrumentRegistry::new(pool).await?;

// Pre-register all symbols from ZIP
for filename in zip.file_names() {
    let symbol = parse_filename(filename)?;
    registry.get_or_create(&symbol, "KRAKEN").await?;
}
```

## Future Enhancements

1. **Resume support**: Track last imported timestamp per symbol
2. **Incremental updates**: Detect new data in updated archives
3. **Compression**: Store with TimescaleDB compression
4. **Partitioning**: Auto-create partitions by month
5. **Side inference**: Attempt to infer trade side from price movement

## Dependencies

```toml
[dependencies]
zip = "0.6"          # ZIP archive handling
csv = "1.3"          # Alternative to manual parsing
rayon = "1.8"        # Parallel processing
indicatif = "0.17"   # Progress bars
```
