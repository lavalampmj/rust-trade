use super::base::{Signal, Strategy};
use crate::data::types::{BarData, BarDataMode, BarType, Timeframe};
use rust_decimal::Decimal;
use std::collections::{HashMap, VecDeque};

pub struct SmaStrategy {
    short_period: usize,
    long_period: usize,
    prices: VecDeque<Decimal>,
    last_signal: Option<Signal>,
}

impl SmaStrategy {
    pub fn new() -> Self {
        Self {
            short_period: 5,
            long_period: 20,
            prices: VecDeque::new(),
            last_signal: None,
        }
    }

    fn calculate_sma(&self, period: usize) -> Option<Decimal> {
        if self.prices.len() < period {
            return None;
        }
        let sum: Decimal = self.prices.iter().rev().take(period).sum();
        Some(sum / Decimal::from(period))
    }
}

impl Strategy for SmaStrategy {
    fn name(&self) -> &str {
        "Simple Moving Average"
    }

    fn on_bar_data(&mut self, bar_data: &BarData) -> Signal {
        // Use close price from OHLC bar
        let current_price = bar_data.ohlc_bar.close;
        let symbol = &bar_data.ohlc_bar.symbol;

        self.prices.push_back(current_price);

        // Keep reasonable history length
        if self.prices.len() > self.long_period * 2 {
            self.prices.pop_front();
        }

        if let (Some(short_sma), Some(long_sma)) = (
            self.calculate_sma(self.short_period),
            self.calculate_sma(self.long_period),
        ) {
            // Golden cross: short MA crosses above long MA
            if short_sma > long_sma && !matches!(self.last_signal, Some(Signal::Buy { .. })) {
                let signal = Signal::Buy {
                    symbol: symbol.clone(),
                    quantity: Decimal::from(100),
                };
                self.last_signal = Some(signal.clone());
                return signal;
            }
            // Death cross: short MA crosses below long MA
            else if short_sma < long_sma && matches!(self.last_signal, Some(Signal::Buy { .. })) {
                let signal = Signal::Sell {
                    symbol: symbol.clone(),
                    quantity: Decimal::from(100),
                };
                self.last_signal = Some(signal.clone());
                return signal;
            }
        }

        Signal::Hold
    }

    fn initialize(&mut self, params: HashMap<String, String>) -> Result<(), String> {
        if let Some(short) = params.get("short_period") {
            self.short_period = short.parse().map_err(|_| "Invalid short_period")?;
        }
        if let Some(long) = params.get("long_period") {
            self.long_period = long.parse().map_err(|_| "Invalid long_period")?;
        }

        if self.short_period >= self.long_period {
            return Err("Short period must be less than long period".to_string());
        }

        println!(
            "SMA Strategy initialized: short={}, long={}",
            self.short_period, self.long_period
        );
        Ok(())
    }

    fn reset(&mut self) {
        self.prices.clear();
        self.last_signal = None;
    }

    fn bar_data_mode(&self) -> BarDataMode {
        BarDataMode::OnCloseBar // Process on bar close
    }

    fn preferred_bar_type(&self) -> BarType {
        BarType::TimeBased(Timeframe::OneMinute)
    }
}
