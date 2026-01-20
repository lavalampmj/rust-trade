use super::base::{Signal, Strategy};
use crate::data::types::{BarData, BarDataMode, BarType, Timeframe};
use crate::series::bars_context::BarsContext;
use crate::series::MaximumBarsLookBack;
use rust_decimal::Decimal;
use std::collections::HashMap;

pub struct RsiStrategy {
    period: usize,
    oversold: Decimal,
    overbought: Decimal,
    last_signal: Option<Signal>,
}

impl RsiStrategy {
    pub fn new() -> Self {
        Self {
            period: 14,
            oversold: Decimal::from(30),
            overbought: Decimal::from(70),
            last_signal: None,
        }
    }

    /// Calculate RSI using BarsContext's close series
    fn calculate_rsi(&self, bars: &mut BarsContext) -> Option<Decimal> {
        // Need at least period + 1 bars to calculate RSI
        if bars.count() < self.period + 1 {
            return None;
        }

        // Register gains/losses series if not already registered
        if !bars.has_series("_rsi_gains") {
            bars.register_series::<Decimal>("_rsi_gains");
            bars.register_series::<Decimal>("_rsi_losses");
        }

        // Calculate current gain/loss from price change
        let change = bars.change()?;
        let (gain, loss) = if change > Decimal::ZERO {
            (change, Decimal::ZERO)
        } else {
            (Decimal::ZERO, -change)
        };

        // Push to tracking series
        bars.push_to_series("_rsi_gains", gain);
        bars.push_to_series("_rsi_losses", loss);

        // Get the gains/losses series
        let gains_series = bars.get_series::<Decimal>("_rsi_gains")?;
        let losses_series = bars.get_series::<Decimal>("_rsi_losses")?;

        // Need enough data in gain/loss series
        if gains_series.count() < self.period {
            return None;
        }

        // Calculate average gain and loss over the period
        let avg_gain: Decimal = (0..self.period)
            .filter_map(|i| gains_series.get(i))
            .cloned()
            .sum::<Decimal>()
            / Decimal::from(self.period);

        let avg_loss: Decimal = (0..self.period)
            .filter_map(|i| losses_series.get(i))
            .cloned()
            .sum::<Decimal>()
            / Decimal::from(self.period);

        // Handle edge case: all losses are zero
        if avg_loss == Decimal::ZERO {
            return Some(Decimal::from(100));
        }

        let rs = avg_gain / avg_loss;
        let rsi = Decimal::from(100) - (Decimal::from(100) / (Decimal::from(1) + rs));

        Some(rsi)
    }
}

impl Strategy for RsiStrategy {
    fn name(&self) -> &str {
        "RSI Strategy"
    }

    fn is_ready(&self, bars: &BarsContext) -> bool {
        // Need period + 1 for price changes (to calculate gains/losses)
        bars.is_ready_for(self.period + 1)
    }

    fn warmup_period(&self) -> usize {
        self.period + 1
    }

    fn on_bar_data(&mut self, _bar_data: &BarData, bars: &mut BarsContext) -> Signal {
        // Defense-in-depth: early return if not ready
        if !self.is_ready(bars) {
            return Signal::Hold;
        }

        // Calculate RSI using the context
        if let Some(rsi) = self.calculate_rsi(bars) {
            let symbol = bars.symbol().to_string();

            // Buy signal when RSI is oversold
            if rsi < self.oversold && !matches!(self.last_signal, Some(Signal::Buy { .. })) {
                let signal = Signal::Buy {
                    symbol,
                    quantity: Decimal::from(100),
                };
                self.last_signal = Some(signal.clone());
                return signal;
            }
            // Sell signal when RSI is overbought
            else if rsi > self.overbought && matches!(self.last_signal, Some(Signal::Buy { .. })) {
                let signal = Signal::Sell {
                    symbol,
                    quantity: Decimal::from(100),
                };
                self.last_signal = Some(signal.clone());
                return signal;
            }
        }

        Signal::Hold
    }

    fn initialize(&mut self, params: HashMap<String, String>) -> Result<(), String> {
        if let Some(period) = params.get("period") {
            self.period = period.parse().map_err(|_| "Invalid period")?;
        }
        if let Some(oversold) = params.get("oversold") {
            self.oversold = oversold.parse().map_err(|_| "Invalid oversold")?;
        }
        if let Some(overbought) = params.get("overbought") {
            self.overbought = overbought.parse().map_err(|_| "Invalid overbought")?;
        }

        if self.oversold >= self.overbought {
            return Err("Oversold level must be less than overbought level".to_string());
        }

        println!(
            "RSI Strategy initialized: period={}, oversold={}, overbought={}",
            self.period, self.oversold, self.overbought
        );
        Ok(())
    }

    fn reset(&mut self) {
        self.last_signal = None;
    }

    fn bar_data_mode(&self) -> BarDataMode {
        BarDataMode::OnCloseBar // Process on bar close
    }

    fn preferred_bar_type(&self) -> BarType {
        BarType::TimeBased(Timeframe::OneDay)
    }

    fn max_bars_lookback(&self) -> MaximumBarsLookBack {
        // Need period + extra for calculations
        MaximumBarsLookBack::Fixed(self.period * 2 + 10)
    }
}
