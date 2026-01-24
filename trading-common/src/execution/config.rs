//! TOML configuration for execution simulation.
//!
//! This module provides configuration structures that can be loaded from
//! TOML files for configuring the simulated exchange and its components.
//!
//! # Example TOML
//! ```toml
//! [exchange]
//! default_venue = "SIMULATED"
//! bar_execution = true
//! reject_stop_orders = false
//!
//! [exchange.latency]
//! model = "fixed"
//! insert_latency_ns = 50_000_000
//!
//! [exchange.fill_model]
//! model = "limit_aware"
//!
//! [exchange.fees]
//! model = "percentage"
//! maker_rate = 0.001
//! taker_rate = 0.001
//! ```

use rust_decimal::Decimal;
use serde::Deserialize;

use super::{
    FixedLatencyModel, LatencyModel, MatchingEngineConfig, NoLatencyModel,
    SimulatedExchangeConfig, VariableLatencyModel,
};
use crate::risk::{FeeModel, FeeTier, HybridFeeModel, PercentageFeeModel, TieredFeeModel, ZeroFeeModel};

/// Root configuration for exchange simulation.
#[derive(Debug, Clone, Deserialize)]
pub struct ExchangeTomlConfig {
    /// Default venue name
    #[serde(default = "default_venue")]
    pub default_venue: String,

    /// Use OHLC bars for order matching
    #[serde(default = "default_true")]
    pub bar_execution: bool,

    /// Use trade ticks for order matching
    #[serde(default = "default_true")]
    pub trade_execution: bool,

    /// Use quote ticks for order matching
    #[serde(default = "default_true")]
    pub quote_execution: bool,

    /// Use L2 order book for matching (requires book data)
    #[serde(default)]
    pub book_execution: bool,

    /// Reject stop orders (some venues don't support)
    #[serde(default)]
    pub reject_stop_orders: bool,

    /// Support Good-Till-Date orders
    #[serde(default = "default_true")]
    pub support_gtd_orders: bool,

    /// Support contingent orders (OTO/OCO/OUO)
    #[serde(default = "default_true")]
    pub support_contingent_orders: bool,

    /// Latency configuration
    #[serde(default)]
    pub latency: LatencyTomlConfig,

    /// Fill model configuration
    #[serde(default)]
    pub fill_model: FillModelTomlConfig,

    /// Matching engine configuration
    #[serde(default)]
    pub matching: MatchingTomlConfig,

    /// Fee model configuration
    #[serde(default)]
    pub fees: FeeTomlConfig,

    /// Contingent order configuration
    #[serde(default)]
    pub contingent: ContingentTomlConfig,
}

fn default_venue() -> String {
    "SIMULATED".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for ExchangeTomlConfig {
    fn default() -> Self {
        Self {
            default_venue: default_venue(),
            bar_execution: true,
            trade_execution: true,
            quote_execution: true,
            book_execution: false,
            reject_stop_orders: false,
            support_gtd_orders: true,
            support_contingent_orders: true,
            latency: LatencyTomlConfig::default(),
            fill_model: FillModelTomlConfig::default(),
            matching: MatchingTomlConfig::default(),
            fees: FeeTomlConfig::default(),
            contingent: ContingentTomlConfig::default(),
        }
    }
}

impl ExchangeTomlConfig {
    /// Build a SimulatedExchangeConfig from this TOML config.
    pub fn build_exchange_config(&self) -> SimulatedExchangeConfig {
        SimulatedExchangeConfig {
            venue: self.default_venue.clone(),
            bar_execution: self.bar_execution,
            trade_execution: self.trade_execution,
            book_execution: self.book_execution,
            liquidity_consumption: self.matching.liquidity_consumption,
            reject_stop_orders: self.reject_stop_orders,
            support_gtd_orders: self.support_gtd_orders,
            support_contingent_orders: self.support_contingent_orders,
            matching_config: self.matching.build_matching_config(),
        }
    }

    /// Build a LatencyModel from this config.
    pub fn build_latency_model(&self) -> Box<dyn LatencyModel> {
        self.latency.build()
    }

    /// Build a FeeModel from this config.
    pub fn build_fee_model(&self) -> Box<dyn FeeModel> {
        self.fees.build()
    }
}

/// Latency model configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct LatencyTomlConfig {
    /// Model type: "none", "fixed", "variable"
    #[serde(default = "default_latency_model")]
    pub model: String,

    /// Fixed latency (nanoseconds) for order submission
    #[serde(default = "default_insert_latency")]
    pub insert_latency_ns: u64,

    /// Fixed latency (nanoseconds) for order modification
    #[serde(default = "default_update_latency")]
    pub update_latency_ns: u64,

    /// Fixed latency (nanoseconds) for order cancellation
    #[serde(default = "default_delete_latency")]
    pub delete_latency_ns: u64,

    /// Base latency for variable model (nanoseconds)
    #[serde(default = "default_base_latency")]
    pub base_latency_ns: u64,

    /// Random jitter for variable model (nanoseconds)
    #[serde(default = "default_jitter")]
    pub jitter_ns: u64,

    /// RNG seed for variable model (0 = random seed)
    #[serde(default)]
    pub seed: u64,
}

fn default_latency_model() -> String {
    "none".to_string()
}

fn default_insert_latency() -> u64 {
    50_000_000 // 50ms
}

fn default_update_latency() -> u64 {
    50_000_000 // 50ms
}

fn default_delete_latency() -> u64 {
    30_000_000 // 30ms
}

fn default_base_latency() -> u64 {
    30_000_000 // 30ms
}

fn default_jitter() -> u64 {
    20_000_000 // 20ms
}

impl Default for LatencyTomlConfig {
    fn default() -> Self {
        Self {
            model: default_latency_model(),
            insert_latency_ns: default_insert_latency(),
            update_latency_ns: default_update_latency(),
            delete_latency_ns: default_delete_latency(),
            base_latency_ns: default_base_latency(),
            jitter_ns: default_jitter(),
            seed: 0,
        }
    }
}

impl LatencyTomlConfig {
    /// Build a LatencyModel from this config.
    pub fn build(&self) -> Box<dyn LatencyModel> {
        match self.model.as_str() {
            "fixed" => {
                // FixedLatencyModel uses single latency for all operations
                Box::new(FixedLatencyModel::new(self.insert_latency_ns))
            }
            "variable" => Box::new(VariableLatencyModel::new(
                self.base_latency_ns,
                self.jitter_ns,
                self.seed,
            )),
            _ => Box::new(NoLatencyModel),
        }
    }
}

/// Fill model configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct FillModelTomlConfig {
    /// Model type: "immediate", "limit_aware", "slippage_aware", "probabilistic"
    #[serde(default = "default_fill_model")]
    pub model: String,

    /// Probability limit fills when price touched (0.0-1.0)
    #[serde(default = "default_prob_fill_on_limit")]
    pub prob_fill_on_limit: f64,

    /// Probability of slippage on market orders (0.0-1.0)
    #[serde(default = "default_prob_slippage")]
    pub prob_slippage: f64,

    /// Maximum slippage in ticks
    #[serde(default = "default_max_slippage_ticks")]
    pub max_slippage_ticks: u32,

    /// RNG seed for reproducibility (0 = random seed)
    #[serde(default)]
    pub seed: u64,

    /// Base slippage percentage for slippage_aware model
    #[serde(default = "default_base_slippage_pct")]
    pub base_slippage_pct: f64,

    /// Additional slippage per unit volume
    #[serde(default)]
    pub volume_impact: f64,

    /// Maximum slippage percentage cap
    #[serde(default = "default_max_slippage_pct")]
    pub max_slippage_pct: f64,
}

fn default_fill_model() -> String {
    "limit_aware".to_string()
}

fn default_prob_fill_on_limit() -> f64 {
    0.8
}

fn default_prob_slippage() -> f64 {
    0.2
}

fn default_max_slippage_ticks() -> u32 {
    2
}

fn default_base_slippage_pct() -> f64 {
    0.001 // 0.1%
}

fn default_max_slippage_pct() -> f64 {
    0.05 // 5%
}

impl Default for FillModelTomlConfig {
    fn default() -> Self {
        Self {
            model: default_fill_model(),
            prob_fill_on_limit: default_prob_fill_on_limit(),
            prob_slippage: default_prob_slippage(),
            max_slippage_ticks: default_max_slippage_ticks(),
            seed: 0,
            base_slippage_pct: default_base_slippage_pct(),
            volume_impact: 0.0,
            max_slippage_pct: default_max_slippage_pct(),
        }
    }
}

/// Matching engine configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct MatchingTomlConfig {
    /// Track consumed liquidity at each price level
    #[serde(default)]
    pub liquidity_consumption: bool,

    /// Reset consumption when new bar arrives
    #[serde(default = "default_true")]
    pub reset_consumption_on_bar: bool,

    /// Add randomness to fill determination
    #[serde(default)]
    pub use_random_fills: bool,

    /// Allow partial fills
    #[serde(default = "default_true")]
    pub partial_fills_enabled: bool,

    /// Fill market orders at bar open (vs close)
    #[serde(default)]
    pub bar_open_fills: bool,

    /// Use high/low to determine if limit touched
    #[serde(default = "default_true")]
    pub use_ohlc_range: bool,
}

impl Default for MatchingTomlConfig {
    fn default() -> Self {
        Self {
            liquidity_consumption: false,
            reset_consumption_on_bar: true,
            use_random_fills: false,
            partial_fills_enabled: true,
            bar_open_fills: false,
            use_ohlc_range: true,
        }
    }
}

impl MatchingTomlConfig {
    /// Build a MatchingEngineConfig from this TOML config.
    pub fn build_matching_config(&self) -> MatchingEngineConfig {
        MatchingEngineConfig {
            bar_execution: true,
            trade_execution: true,
            quote_execution: true,
            book_execution: false,
            liquidity_consumption: self.liquidity_consumption,
            reject_stop_orders: false,
            use_random_fills: self.use_random_fills,
            bar_open_fills: self.bar_open_fills,
            use_ohlc_range: self.use_ohlc_range,
            partial_fills_enabled: self.partial_fills_enabled,
            min_fill_pct: Decimal::new(1, 1), // 0.1 = 10%
        }
    }
}

/// Fee model configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct FeeTomlConfig {
    /// Model type: "percentage", "tiered", "fixed", "hybrid", "zero"
    #[serde(default = "default_fee_model")]
    pub model: String,

    /// Maker fee rate (for percentage model)
    #[serde(default = "default_maker_rate")]
    pub maker_rate: f64,

    /// Taker fee rate (for percentage model)
    #[serde(default = "default_taker_rate")]
    pub taker_rate: f64,

    /// Fixed fee per fill
    #[serde(default)]
    pub fixed_per_fill: f64,

    /// Minimum fee
    #[serde(default)]
    pub min_fee: f64,

    /// Maximum fee cap
    #[serde(default = "default_max_fee")]
    pub max_fee: f64,

    /// Volume thresholds for tiered model
    #[serde(default)]
    pub tier_volume_thresholds: Vec<f64>,

    /// Maker rates per tier
    #[serde(default)]
    pub tier_maker_rates: Vec<f64>,

    /// Taker rates per tier
    #[serde(default)]
    pub tier_taker_rates: Vec<f64>,
}

fn default_fee_model() -> String {
    "percentage".to_string()
}

fn default_maker_rate() -> f64 {
    0.001 // 0.1%
}

fn default_taker_rate() -> f64 {
    0.001 // 0.1%
}

fn default_max_fee() -> f64 {
    1_000_000.0
}

impl Default for FeeTomlConfig {
    fn default() -> Self {
        Self {
            model: default_fee_model(),
            maker_rate: default_maker_rate(),
            taker_rate: default_taker_rate(),
            fixed_per_fill: 0.0,
            min_fee: 0.0,
            max_fee: default_max_fee(),
            tier_volume_thresholds: Vec::new(),
            tier_maker_rates: Vec::new(),
            tier_taker_rates: Vec::new(),
        }
    }
}

impl FeeTomlConfig {
    /// Build a FeeModel from this config.
    pub fn build(&self) -> Box<dyn FeeModel> {
        match self.model.as_str() {
            "zero" => Box::new(ZeroFeeModel),
            "percentage" => Box::new(PercentageFeeModel::new(
                Decimal::try_from(self.maker_rate).unwrap_or_default(),
                Decimal::try_from(self.taker_rate).unwrap_or_default(),
            )),
            "tiered" => {
                let num_tiers = self.tier_volume_thresholds.len()
                    .min(self.tier_maker_rates.len())
                    .min(self.tier_taker_rates.len());

                if num_tiers > 0 {
                    let tiers: Vec<FeeTier> = (0..num_tiers)
                        .map(|i| {
                            FeeTier::new(
                                format!("Tier {}", i + 1),
                                Decimal::try_from(self.tier_volume_thresholds[i]).unwrap_or_default(),
                                Decimal::try_from(self.tier_maker_rates[i]).unwrap_or_default(),
                                Decimal::try_from(self.tier_taker_rates[i]).unwrap_or_default(),
                            )
                        })
                        .collect();
                    Box::new(TieredFeeModel::new(tiers))
                } else {
                    // Fallback to percentage
                    Box::new(PercentageFeeModel::new(
                        Decimal::try_from(self.maker_rate).unwrap_or_default(),
                        Decimal::try_from(self.taker_rate).unwrap_or_default(),
                    ))
                }
            }
            "hybrid" => Box::new(HybridFeeModel::new(
                Decimal::try_from(self.maker_rate).unwrap_or_default(),
                Decimal::try_from(self.taker_rate).unwrap_or_default(),
                Decimal::try_from(self.fixed_per_fill).unwrap_or_default(),
            )),
            _ => Box::new(PercentageFeeModel::new(
                Decimal::try_from(self.maker_rate).unwrap_or_default(),
                Decimal::try_from(self.taker_rate).unwrap_or_default(),
            )),
        }
    }
}

/// Contingent order management configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ContingentTomlConfig {
    /// Auto-manage OTO/OCO/OUO orders
    #[serde(default = "default_true")]
    pub manage_contingent_orders: bool,

    /// Auto-cancel expired GTD orders
    #[serde(default = "default_true")]
    pub manage_gtd_expiry: bool,

    /// Delay before submitting OTO children (nanoseconds)
    #[serde(default)]
    pub oto_submit_delay_ns: u64,

    /// Delay before canceling OCO siblings (nanoseconds)
    #[serde(default)]
    pub oco_cancel_delay_ns: u64,
}

impl Default for ContingentTomlConfig {
    fn default() -> Self {
        Self {
            manage_contingent_orders: true,
            manage_gtd_expiry: true,
            oto_submit_delay_ns: 0,
            oco_cancel_delay_ns: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::Order;

    #[test]
    fn test_default_config() {
        let config = ExchangeTomlConfig::default();

        assert_eq!(config.default_venue, "SIMULATED");
        assert!(config.bar_execution);
        assert!(!config.reject_stop_orders);
        assert_eq!(config.latency.model, "none");
        assert_eq!(config.fill_model.model, "limit_aware");
        assert_eq!(config.fees.model, "percentage");
    }

    #[test]
    fn test_build_exchange_config() {
        let config = ExchangeTomlConfig::default();
        let exchange_config = config.build_exchange_config();

        assert!(exchange_config.bar_execution);
        assert!(!exchange_config.reject_stop_orders);
    }

    #[test]
    fn test_build_no_latency_model() {
        let config = LatencyTomlConfig {
            model: "none".to_string(),
            ..Default::default()
        };

        let model = config.build();
        assert_eq!(model.insert_latency_nanos(), 0);
    }

    #[test]
    fn test_build_fixed_latency_model() {
        let config = LatencyTomlConfig {
            model: "fixed".to_string(),
            insert_latency_ns: 100_000_000, // 100ms
            ..Default::default()
        };

        let model = config.build();
        assert_eq!(model.insert_latency_nanos(), 100_000_000);
    }

    #[test]
    fn test_build_variable_latency_model() {
        let config = LatencyTomlConfig {
            model: "variable".to_string(),
            base_latency_ns: 50_000_000,
            jitter_ns: 25_000_000,
            seed: 42,
            ..Default::default()
        };

        let model = config.build();
        let latency = model.insert_latency_nanos();
        // Should be in range [base, base + jitter]
        assert!(latency >= 50_000_000);
        assert!(latency <= 75_000_000);
    }

    #[test]
    fn test_build_percentage_fee_model() {
        let config = FeeTomlConfig {
            model: "percentage".to_string(),
            maker_rate: 0.001,
            taker_rate: 0.002,
            ..Default::default()
        };

        let _model = config.build();
        // Model created successfully
    }

    #[test]
    fn test_build_zero_fee_model() {
        let config = FeeTomlConfig {
            model: "zero".to_string(),
            ..Default::default()
        };

        let model = config.build();
        // Calculate fee should return 0
        let fee = model.calculate_fee(
            Decimal::from(100),
            Decimal::from(50000),
            &Order::market("TEST", crate::orders::OrderSide::Buy, Decimal::from(1))
                .build()
                .unwrap(),
            crate::orders::LiquiditySide::Taker,
        );
        assert_eq!(fee, Decimal::ZERO);
    }

    #[test]
    fn test_parse_toml() {
        let toml_str = r#"
            default_venue = "BACKTEST"
            bar_execution = true
            reject_stop_orders = false

            [latency]
            model = "fixed"
            insert_latency_ns = 100000000

            [fees]
            model = "percentage"
            maker_rate = 0.0005
            taker_rate = 0.001
        "#;

        let config: ExchangeTomlConfig = toml::from_str(toml_str).unwrap();

        assert_eq!(config.default_venue, "BACKTEST");
        assert!(config.bar_execution);
        assert_eq!(config.latency.model, "fixed");
        assert_eq!(config.latency.insert_latency_ns, 100_000_000);
        assert_eq!(config.fees.maker_rate, 0.0005);
    }
}
