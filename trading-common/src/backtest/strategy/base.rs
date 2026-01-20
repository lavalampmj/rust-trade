use crate::data::types::{BarData, BarDataMode, BarType, Timeframe};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Signal {
    Buy { symbol: String, quantity: Decimal },
    Sell { symbol: String, quantity: Decimal },
    Hold,
}

pub trait Strategy: Send + Sync {
    fn name(&self) -> &str;

    /// Unified bar data processing method
    ///
    /// This is the primary method for processing market data.
    /// Receives BarData which contains:
    /// - current_tick: Optional tick (Some for OnEachTick/OnPriceMove, None for OnCloseBar)
    /// - ohlc_bar: Current OHLC bar state (always present)
    /// - metadata: Bar state information (first tick, closed, synthetic, etc.)
    fn on_bar_data(&mut self, bar_data: &BarData) -> Signal;

    /// Initialize strategy with parameters
    fn initialize(&mut self, params: HashMap<String, String>) -> Result<(), String>;

    /// Reset strategy state for new backtest
    fn reset(&mut self) {
        // Default implementation does nothing
        // Strategies can override if needed
    }

    /// Specify the operational mode for bar data processing
    ///
    /// - OnEachTick: Fire on every tick (default)
    /// - OnPriceMove: Fire only when price changes
    /// - OnCloseBar: Fire only when bar closes
    fn bar_data_mode(&self) -> BarDataMode {
        BarDataMode::OnEachTick // Default
    }

    /// Specify the preferred bar type for this strategy
    ///
    /// Returns the type of bars this strategy wants to process
    fn preferred_bar_type(&self) -> BarType {
        BarType::TimeBased(Timeframe::OneMinute) // Default
    }
}
