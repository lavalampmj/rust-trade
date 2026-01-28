pub mod backfill;
pub mod backfill_config;
pub mod cache;
pub mod dbn_types;
pub mod events;
pub mod gap_detection;
pub mod instrument_registry;
pub mod orderbook;
pub mod quotes;
pub mod repository;
pub mod sequence;
pub mod symbol_repository;
pub mod types;

// Re-export DBN types as the canonical format
pub use dbn_types::{
    // Core DBN record types
    BboMsg, BidAskPair, MboMsg, Mbp10Msg, Mbp1Msg, TradeMsg,
    // Extension traits for convenient access
    BboMsgExt, MboMsgExt, Mbp10MsgExt, Mbp1MsgExt, TradeMsgExt,
    // Compatible enum types
    BookActionCompat, BookSideCompat, TradeSideCompat,
    // Creation functions
    create_bbo_msg, create_bbo_msg_from_decimals, create_mbo_msg, create_trade_msg,
    create_trade_msg_from_decimals, create_trade_msg_with_instrument_id,
    // Conversion functions
    datetime_to_nanos, decimal_to_dbn_price, decimal_to_dbn_size, nanos_to_datetime,
    symbol_to_instrument_id,
    // Constants and re-exports
    DbnSide, FlagSet, RecordHeader, CUSTOM_PUBLISHER_ID, DBN_PRICE_SCALE,
};

// Re-export quote and order book types
pub use orderbook::{
    BookAction, BookDepth, BookLevel, BookSide, OrderBook, OrderBookDelta, OrderBookDeltas,
};
pub use quotes::{QuoteStats, QuoteStatsBuilder, QuoteTick};

// Re-export symbol repository
pub use symbol_repository::SymbolRepository;

// Re-export market data events
pub use events::{MarketDataEvent, MarketDataEventDbn, MarketDataType};

// Re-export sequence generator
pub use sequence::SequenceGenerator;

// Re-export instrument registry
pub use instrument_registry::{get_global_registry, InstrumentRegistry};
