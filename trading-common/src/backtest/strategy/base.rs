use crate::data::types::{BarData, BarDataMode, BarType, OHLCData, TickData, Timeframe};
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

    // ============================================================
    // NEW UNIFIED INTERFACE
    // ============================================================

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

    // ============================================================
    // DEPRECATED METHODS (for backward compatibility)
    // ============================================================

    /// DEPRECATED: Use on_bar_data() instead
    ///
    /// This method is kept for backward compatibility.
    /// Default implementation wraps the tick into BarData and calls on_bar_data()
    #[deprecated(
        since = "0.2.0",
        note = "Use on_bar_data() instead for unified interface"
    )]
    fn on_tick(&mut self, tick: &TickData) -> Signal {
        let bar_data = BarData::from_single_tick(tick);
        self.on_bar_data(&bar_data)
    }

    /// DEPRECATED: Use on_bar_data() instead
    ///
    /// This method is kept for backward compatibility.
    /// Default implementation wraps the OHLC into BarData and calls on_bar_data()
    #[deprecated(
        since = "0.2.0",
        note = "Use on_bar_data() instead for unified interface"
    )]
    fn on_ohlc(&mut self, ohlc: &OHLCData) -> Signal {
        let bar_data = BarData::from_ohlc(ohlc);
        self.on_bar_data(&bar_data)
    }

    /// DEPRECATED: Use preferred_bar_type() instead
    #[deprecated(since = "0.2.0", note = "Use preferred_bar_type() instead")]
    fn supports_ohlc(&self) -> bool {
        false
    }

    /// DEPRECATED: Use preferred_bar_type() instead
    #[deprecated(since = "0.2.0", note = "Use preferred_bar_type() instead")]
    fn preferred_timeframe(&self) -> Option<Timeframe> {
        None
    }
}
