//! Fee models for commission calculation.
//!
//! This module provides fee calculation abstractions for different
//! commission structures across venues and instrument types.

use crate::orders::{LiquiditySide, Order, OrderType};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Trait for calculating trading fees/commissions.
///
/// Implement this trait to define custom fee structures for different
/// venues, account types, or promotional tiers.
pub trait FeeModel: Send + Sync + fmt::Debug {
    /// Calculate the fee for an order fill.
    ///
    /// # Arguments
    /// * `fill_qty` - Quantity being filled
    /// * `fill_price` - Price of the fill
    /// * `order` - The order being filled (for context like order type)
    /// * `liquidity_side` - Whether this is maker or taker
    ///
    /// # Returns
    /// The fee amount in quote currency
    fn calculate_fee(
        &self,
        fill_qty: Decimal,
        fill_price: Decimal,
        order: &Order,
        liquidity_side: LiquiditySide,
    ) -> Decimal;

    /// Get the maker fee rate
    fn maker_rate(&self) -> Decimal;

    /// Get the taker fee rate
    fn taker_rate(&self) -> Decimal;

    /// Get fee rate for a specific liquidity side
    fn rate_for_side(&self, side: LiquiditySide) -> Decimal {
        match side {
            LiquiditySide::Maker => self.maker_rate(),
            LiquiditySide::Taker => self.taker_rate(),
            LiquiditySide::None => self.taker_rate(), // Default to taker
        }
    }
}

/// Simple percentage-based fee model.
///
/// Calculates fees as a percentage of notional value with
/// separate maker and taker rates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PercentageFeeModel {
    /// Maker fee rate (e.g., 0.001 for 0.1%)
    pub maker_rate: Decimal,
    /// Taker fee rate (e.g., 0.001 for 0.1%)
    pub taker_rate: Decimal,
}

impl PercentageFeeModel {
    /// Create a new percentage fee model
    pub fn new(maker_rate: Decimal, taker_rate: Decimal) -> Self {
        Self {
            maker_rate,
            taker_rate,
        }
    }

    /// Create with equal maker/taker rates
    pub fn flat(rate: Decimal) -> Self {
        Self::new(rate, rate)
    }

    /// Create a zero-fee model
    pub fn zero() -> Self {
        Self::new(Decimal::ZERO, Decimal::ZERO)
    }

    /// Create Binance spot default fees (0.1% maker/taker)
    pub fn binance_spot() -> Self {
        Self::new(Decimal::new(1, 3), Decimal::new(1, 3))
    }

    /// Create Binance spot VIP0 fees with BNB discount
    pub fn binance_spot_bnb() -> Self {
        Self::new(Decimal::new(75, 5), Decimal::new(75, 5)) // 0.075%
    }

    /// Create Binance futures default fees
    pub fn binance_futures() -> Self {
        Self::new(Decimal::new(2, 4), Decimal::new(4, 4)) // 0.02% / 0.04%
    }
}

impl FeeModel for PercentageFeeModel {
    fn calculate_fee(
        &self,
        fill_qty: Decimal,
        fill_price: Decimal,
        _order: &Order,
        liquidity_side: LiquiditySide,
    ) -> Decimal {
        let notional = fill_qty * fill_price;
        let rate = self.rate_for_side(liquidity_side);
        notional * rate
    }

    fn maker_rate(&self) -> Decimal {
        self.maker_rate
    }

    fn taker_rate(&self) -> Decimal {
        self.taker_rate
    }
}

impl Default for PercentageFeeModel {
    fn default() -> Self {
        Self::binance_spot()
    }
}

/// Tiered fee model based on volume or VIP level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TieredFeeModel {
    /// Tiers sorted by minimum volume (ascending)
    pub tiers: Vec<FeeTier>,
    /// Current tier index
    current_tier: usize,
}

/// A single fee tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeTier {
    /// Tier name (e.g., "VIP0", "VIP1")
    pub name: String,
    /// Minimum 30-day volume for this tier
    pub min_volume: Decimal,
    /// Maker fee rate
    pub maker_rate: Decimal,
    /// Taker fee rate
    pub taker_rate: Decimal,
}

impl FeeTier {
    /// Create a new fee tier
    pub fn new(
        name: impl Into<String>,
        min_volume: Decimal,
        maker_rate: Decimal,
        taker_rate: Decimal,
    ) -> Self {
        Self {
            name: name.into(),
            min_volume,
            maker_rate,
            taker_rate,
        }
    }
}

impl TieredFeeModel {
    /// Create a new tiered fee model
    pub fn new(tiers: Vec<FeeTier>) -> Self {
        Self {
            tiers,
            current_tier: 0,
        }
    }

    /// Create Binance spot tiers
    pub fn binance_spot_tiers() -> Self {
        Self::new(vec![
            FeeTier::new("VIP0", Decimal::ZERO, Decimal::new(1, 3), Decimal::new(1, 3)),
            FeeTier::new("VIP1", Decimal::new(1_000_000, 0), Decimal::new(9, 4), Decimal::new(1, 3)),
            FeeTier::new("VIP2", Decimal::new(5_000_000, 0), Decimal::new(8, 4), Decimal::new(1, 3)),
            FeeTier::new("VIP3", Decimal::new(20_000_000, 0), Decimal::new(7, 4), Decimal::new(9, 4)),
        ])
    }

    /// Set the current tier based on volume
    pub fn set_tier_by_volume(&mut self, volume_30d: Decimal) {
        self.current_tier = 0;
        for (i, tier) in self.tiers.iter().enumerate() {
            if volume_30d >= tier.min_volume {
                self.current_tier = i;
            } else {
                break;
            }
        }
    }

    /// Set the current tier by index
    pub fn set_tier(&mut self, tier_index: usize) {
        if tier_index < self.tiers.len() {
            self.current_tier = tier_index;
        }
    }

    /// Get the current tier
    pub fn current_tier(&self) -> &FeeTier {
        &self.tiers[self.current_tier]
    }
}

impl FeeModel for TieredFeeModel {
    fn calculate_fee(
        &self,
        fill_qty: Decimal,
        fill_price: Decimal,
        _order: &Order,
        liquidity_side: LiquiditySide,
    ) -> Decimal {
        let notional = fill_qty * fill_price;
        let rate = self.rate_for_side(liquidity_side);
        notional * rate
    }

    fn maker_rate(&self) -> Decimal {
        self.current_tier().maker_rate
    }

    fn taker_rate(&self) -> Decimal {
        self.current_tier().taker_rate
    }
}

/// Fixed fee model (flat fee per order/fill).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixedFeeModel {
    /// Fixed fee per fill
    pub fee_per_fill: Decimal,
    /// Minimum fee
    pub min_fee: Decimal,
    /// Maximum fee (None = no max)
    pub max_fee: Option<Decimal>,
}

impl FixedFeeModel {
    /// Create a new fixed fee model
    pub fn new(fee_per_fill: Decimal) -> Self {
        Self {
            fee_per_fill,
            min_fee: Decimal::ZERO,
            max_fee: None,
        }
    }

    /// Set minimum fee
    pub fn with_min(mut self, min: Decimal) -> Self {
        self.min_fee = min;
        self
    }

    /// Set maximum fee
    pub fn with_max(mut self, max: Decimal) -> Self {
        self.max_fee = Some(max);
        self
    }
}

impl FeeModel for FixedFeeModel {
    fn calculate_fee(
        &self,
        _fill_qty: Decimal,
        _fill_price: Decimal,
        _order: &Order,
        _liquidity_side: LiquiditySide,
    ) -> Decimal {
        let mut fee = self.fee_per_fill;
        fee = fee.max(self.min_fee);
        if let Some(max) = self.max_fee {
            fee = fee.min(max);
        }
        fee
    }

    fn maker_rate(&self) -> Decimal {
        Decimal::ZERO // Not applicable for fixed fee
    }

    fn taker_rate(&self) -> Decimal {
        Decimal::ZERO // Not applicable for fixed fee
    }
}

/// Hybrid fee model combining percentage and fixed fees.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridFeeModel {
    /// Percentage component
    pub percentage: PercentageFeeModel,
    /// Fixed component per fill
    pub fixed_per_fill: Decimal,
    /// Minimum total fee
    pub min_fee: Decimal,
    /// Maximum total fee (None = no max)
    pub max_fee: Option<Decimal>,
}

impl HybridFeeModel {
    /// Create a new hybrid fee model
    pub fn new(
        maker_rate: Decimal,
        taker_rate: Decimal,
        fixed_per_fill: Decimal,
    ) -> Self {
        Self {
            percentage: PercentageFeeModel::new(maker_rate, taker_rate),
            fixed_per_fill,
            min_fee: Decimal::ZERO,
            max_fee: None,
        }
    }

    /// Set minimum fee
    pub fn with_min(mut self, min: Decimal) -> Self {
        self.min_fee = min;
        self
    }

    /// Set maximum fee
    pub fn with_max(mut self, max: Decimal) -> Self {
        self.max_fee = Some(max);
        self
    }
}

impl FeeModel for HybridFeeModel {
    fn calculate_fee(
        &self,
        fill_qty: Decimal,
        fill_price: Decimal,
        order: &Order,
        liquidity_side: LiquiditySide,
    ) -> Decimal {
        let percentage_fee = self.percentage.calculate_fee(fill_qty, fill_price, order, liquidity_side);
        let mut total = percentage_fee + self.fixed_per_fill;
        total = total.max(self.min_fee);
        if let Some(max) = self.max_fee {
            total = total.min(max);
        }
        total
    }

    fn maker_rate(&self) -> Decimal {
        self.percentage.maker_rate
    }

    fn taker_rate(&self) -> Decimal {
        self.percentage.taker_rate
    }
}

/// Zero fee model for backtesting without fees.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ZeroFeeModel;

impl FeeModel for ZeroFeeModel {
    fn calculate_fee(
        &self,
        _fill_qty: Decimal,
        _fill_price: Decimal,
        _order: &Order,
        _liquidity_side: LiquiditySide,
    ) -> Decimal {
        Decimal::ZERO
    }

    fn maker_rate(&self) -> Decimal {
        Decimal::ZERO
    }

    fn taker_rate(&self) -> Decimal {
        Decimal::ZERO
    }
}

/// Infer liquidity side from order type.
///
/// This is a heuristic - actual liquidity side depends on market state at fill time.
pub fn infer_liquidity_side(order_type: OrderType) -> LiquiditySide {
    match order_type {
        OrderType::Market => LiquiditySide::Taker,
        OrderType::Limit => LiquiditySide::Maker, // Assume resting on book
        OrderType::Stop | OrderType::StopLimit => LiquiditySide::Taker,
        OrderType::TrailingStop => LiquiditySide::Taker,
        OrderType::MarketToLimit => LiquiditySide::Taker, // Initial fill is taker
        OrderType::LimitIfTouched => LiquiditySide::Taker,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::{OrderBuilder, OrderSide, OrderType};
    use rust_decimal_macros::dec;

    fn create_test_order() -> Order {
        OrderBuilder::new(OrderType::Market, "BTCUSDT", OrderSide::Buy, dec!(1))
            .build()
            .unwrap()
    }

    #[test]
    fn test_percentage_fee_model() {
        let model = PercentageFeeModel::new(dec!(0.001), dec!(0.002));
        let order = create_test_order();

        // Taker fee: 1 * 50000 * 0.002 = 100
        let fee = model.calculate_fee(dec!(1), dec!(50000), &order, LiquiditySide::Taker);
        assert_eq!(fee, dec!(100));

        // Maker fee: 1 * 50000 * 0.001 = 50
        let fee = model.calculate_fee(dec!(1), dec!(50000), &order, LiquiditySide::Maker);
        assert_eq!(fee, dec!(50));
    }

    #[test]
    fn test_percentage_fee_presets() {
        let binance = PercentageFeeModel::binance_spot();
        assert_eq!(binance.maker_rate, dec!(0.001));
        assert_eq!(binance.taker_rate, dec!(0.001));

        let futures = PercentageFeeModel::binance_futures();
        assert_eq!(futures.maker_rate, dec!(0.0002));
        assert_eq!(futures.taker_rate, dec!(0.0004));
    }

    #[test]
    fn test_tiered_fee_model() {
        let mut model = TieredFeeModel::binance_spot_tiers();

        // Initially at VIP0
        assert_eq!(model.maker_rate(), dec!(0.001));

        // Set to VIP3 volume
        model.set_tier_by_volume(dec!(25_000_000));
        assert_eq!(model.maker_rate(), dec!(0.0007));
        assert_eq!(model.taker_rate(), dec!(0.0009));
    }

    #[test]
    fn test_fixed_fee_model() {
        let model = FixedFeeModel::new(dec!(1.5))
            .with_min(dec!(1))
            .with_max(dec!(10));

        let order = create_test_order();
        let fee = model.calculate_fee(dec!(100), dec!(50000), &order, LiquiditySide::Taker);
        assert_eq!(fee, dec!(1.5));
    }

    #[test]
    fn test_hybrid_fee_model() {
        let model = HybridFeeModel::new(dec!(0.001), dec!(0.001), dec!(0.5));
        let order = create_test_order();

        // Fee: 1 * 1000 * 0.001 + 0.5 = 1.5
        let fee = model.calculate_fee(dec!(1), dec!(1000), &order, LiquiditySide::Taker);
        assert_eq!(fee, dec!(1.5));
    }

    #[test]
    fn test_zero_fee_model() {
        let model = ZeroFeeModel;
        let order = create_test_order();

        let fee = model.calculate_fee(dec!(100), dec!(50000), &order, LiquiditySide::Taker);
        assert_eq!(fee, Decimal::ZERO);
    }

    #[test]
    fn test_infer_liquidity_side() {
        assert_eq!(infer_liquidity_side(OrderType::Market), LiquiditySide::Taker);
        assert_eq!(infer_liquidity_side(OrderType::Limit), LiquiditySide::Maker);
        assert_eq!(infer_liquidity_side(OrderType::Stop), LiquiditySide::Taker);
    }
}
