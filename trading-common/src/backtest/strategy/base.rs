use crate::data::types::{BarData, BarDataMode, BarType, Timeframe};
use crate::series::bars_context::BarsContext;
use crate::series::MaximumBarsLookBack;
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

    /// Unified bar data processing method with BarsContext
    ///
    /// This is the primary method for processing market data.
    ///
    /// # Arguments
    /// - `bar_data`: Current bar information including:
    ///   - current_tick: Optional tick (Some for OnEachTick/OnPriceMove, None for OnCloseBar)
    ///   - ohlc_bar: Current OHLC bar state (always present)
    ///   - metadata: Bar state information (first tick, closed, synthetic, etc.)
    /// - `bars`: BarsContext providing synchronized access to:
    ///   - OHLCV series with reverse indexing: `bars.close[0]` (current), `bars.close[1]` (previous)
    ///   - Built-in helpers: `bars.sma(period)`, `bars.highest_high(period)`, etc.
    ///   - Custom series registration for indicators
    ///
    /// # Example
    /// ```ignore
    /// fn on_bar_data(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Signal {
    ///     // Access current and historical prices
    ///     let current_close = bars.close[0];
    ///     let prev_close = bars.close[1];
    ///
    ///     // Use built-in indicators
    ///     if let (Some(sma20), Some(sma50)) = (bars.sma(20), bars.sma(50)) {
    ///         if sma20 > sma50 {
    ///             return Signal::Buy { symbol: bars.symbol().to_string(), quantity: 1.into() };
    ///         }
    ///     }
    ///     Signal::Hold
    /// }
    /// ```
    fn on_bar_data(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Signal;

    /// Initialize strategy with parameters
    fn initialize(&mut self, params: HashMap<String, String>) -> Result<(), String>;

    /// Reset strategy state for new backtest
    fn reset(&mut self) {
        // Default implementation does nothing
        // Strategies can override if needed
    }

    // ========================================================================
    // Warmup / Ready State (QuantConnect Lean-style)
    // ========================================================================

    /// Check if strategy has enough data to generate valid signals.
    ///
    /// Returns true when all required indicators are warmed up.
    /// **REQUIRED** - no default implementation. Strategies must explicitly
    /// consider their warmup requirements.
    ///
    /// This implements the QuantConnect Lean-style `is_ready` pattern where
    /// each strategy knows its own readiness state based on its indicators.
    ///
    /// # Arguments
    /// - `bars`: BarsContext to check for data availability
    ///
    /// # Example
    /// ```ignore
    /// fn is_ready(&self, bars: &BarsContext) -> bool {
    ///     // Ready when we have enough data for our longest indicator
    ///     bars.is_ready_for(self.long_period)
    /// }
    /// ```
    fn is_ready(&self, bars: &BarsContext) -> bool;

    /// Return minimum bars needed before is_ready() can return true.
    ///
    /// Used by the framework for progress indication and optimization.
    /// **REQUIRED** - must match the logic in is_ready().
    ///
    /// # Example
    /// ```ignore
    /// fn warmup_period(&self) -> usize {
    ///     self.long_period // Longest indicator period
    /// }
    /// ```
    fn warmup_period(&self) -> usize;

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

    /// Specify the maximum bars lookback for series data
    ///
    /// This determines how much historical data is kept in BarsContext.
    /// - Fixed(256): Default, keeps last 256 bars (FIFO eviction)
    /// - Infinite: No eviction, grows unbounded (use with caution)
    fn max_bars_lookback(&self) -> MaximumBarsLookBack {
        MaximumBarsLookBack::Fixed(256) // Default
    }
}
