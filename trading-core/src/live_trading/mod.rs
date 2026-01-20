pub mod ohlc_generator;
pub mod paper_trading;

pub use ohlc_generator::RealtimeOHLCGenerator;
pub use paper_trading::PaperTradingProcessor;

#[cfg(test)]
mod paper_trading_tests;
