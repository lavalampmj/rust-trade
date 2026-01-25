pub mod backfill;
pub mod backfill_config;
pub mod cache;
pub mod dbn_types;
pub mod events;
pub mod gap_detection;
pub mod orderbook;
pub mod quotes;
pub mod repository;
pub mod sequence;
pub mod symbol_repository;
pub mod types;

// Re-export DBN types as the canonical format
pub use dbn_types::{
    create_trade_msg, create_trade_msg_from_decimals, symbol_to_instrument_id, DbnSide, FlagSet,
    RecordHeader, TradeMsg, TradeMsgExt, TradeSideCompat, CUSTOM_PUBLISHER_ID, DBN_PRICE_SCALE,
};

// Re-export quote and order book types
pub use orderbook::{
    BookAction, BookDepth, BookLevel, BookSide, OrderBook, OrderBookDelta, OrderBookDeltas,
};
pub use quotes::{QuoteStats, QuoteStatsBuilder, QuoteTick};

// Re-export symbol repository
pub use symbol_repository::SymbolRepository;

// Re-export market data events
pub use events::{MarketDataEvent, MarketDataType};

// Re-export sequence generator
pub use sequence::SequenceGenerator;
