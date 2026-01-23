use super::base::Strategy;
use crate::data::types::{BarData, BarDataMode, BarType, Timeframe};
use crate::orders::{Order, OrderSide};
use crate::series::bars_context::BarsContext;
use crate::series::MaximumBarsLookBack;
use rust_decimal::Decimal;
use std::collections::HashMap;

pub struct SmaStrategy {
    short_period: usize,
    long_period: usize,
    last_side: Option<OrderSide>,
    pending_orders: Vec<Order>,
}

impl SmaStrategy {
    pub fn new() -> Self {
        Self {
            short_period: 5,
            long_period: 20,
            last_side: None,
            pending_orders: Vec::new(),
        }
    }
}

impl Strategy for SmaStrategy {
    fn name(&self) -> &str {
        "Simple Moving Average"
    }

    fn is_ready(&self, bars: &BarsContext) -> bool {
        // Both SMAs must have enough data (long_period is the limiting factor)
        bars.is_ready_for(self.long_period)
    }

    fn warmup_period(&self) -> usize {
        self.long_period
    }

    fn on_bar_data(&mut self, _bar_data: &BarData, bars: &mut BarsContext) {
        self.pending_orders.clear();

        // Defense-in-depth: early return if not ready
        // (Engine also checks, but strategy can check for explicit handling)
        if !self.is_ready(bars) {
            return;
        }

        // Use BarsContext's built-in SMA calculation
        // Since we checked is_ready(), these are guaranteed to be Some
        let short_sma = bars.sma(self.short_period);
        let long_sma = bars.sma(self.long_period);

        if let (Some(short), Some(long)) = (short_sma, long_sma) {
            // Golden cross: short MA crosses above long MA
            if short > long && self.last_side != Some(OrderSide::Buy) {
                if let Ok(order) = Order::market(bars.symbol(), OrderSide::Buy, Decimal::from(100))
                    .build()
                {
                    self.pending_orders.push(order);
                    self.last_side = Some(OrderSide::Buy);
                }
            }
            // Death cross: short MA crosses below long MA
            else if short < long && self.last_side == Some(OrderSide::Buy) {
                if let Ok(order) = Order::market(bars.symbol(), OrderSide::Sell, Decimal::from(100))
                    .build()
                {
                    self.pending_orders.push(order);
                    self.last_side = Some(OrderSide::Sell);
                }
            }
        }
    }

    fn get_orders(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) -> Vec<Order> {
        std::mem::take(&mut self.pending_orders)
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
        self.last_side = None;
        self.pending_orders.clear();
    }

    fn bar_data_mode(&self) -> BarDataMode {
        BarDataMode::OnCloseBar // Process on bar close
    }

    fn preferred_bar_type(&self) -> BarType {
        BarType::TimeBased(Timeframe::OneMinute)
    }

    fn max_bars_lookback(&self) -> MaximumBarsLookBack {
        // Need at least long_period * 2 for smooth operation
        MaximumBarsLookBack::Fixed(self.long_period * 2)
    }
}
