// Transform Registry - Dynamic transform management for strategies
//
// Provides runtime registration and lookup of transforms, enabling
// strategies to manage multiple indicators with automatic update ordering.

use std::collections::HashMap;

use super::{BarsContext, Transform};
use rust_decimal::Decimal;

/// Type-erased transform wrapper for registry storage.
///
/// Enables storing transforms of different concrete types together.
pub trait TransformDyn: Send + Sync {
    /// Get the transform name
    #[allow(dead_code)]
    fn name_dyn(&self) -> &str;

    /// Get the warmup period
    fn warmup_period_dyn(&self) -> usize;

    /// Check if ready to produce output
    fn is_ready_dyn(&self, bars: &BarsContext) -> bool;

    /// Compute and return the value as Option<Decimal>
    /// Returns None if not ready or if transform doesn't output Decimal
    fn compute_decimal(&mut self, bars: &BarsContext) -> Option<Decimal>;

    /// Reset the transform
    fn reset_dyn(&mut self);

    /// Get the current value as Option<Decimal>
    fn current_decimal(&self) -> Option<Decimal>;
}

// Implement TransformDyn for all Decimal transforms
impl<T: Transform<Output = Decimal>> TransformDyn for T {
    fn name_dyn(&self) -> &str {
        self.name()
    }

    fn warmup_period_dyn(&self) -> usize {
        self.warmup_period()
    }

    fn is_ready_dyn(&self, bars: &BarsContext) -> bool {
        self.is_ready(bars)
    }

    fn compute_decimal(&mut self, bars: &BarsContext) -> Option<Decimal> {
        self.compute(bars)
    }

    fn reset_dyn(&mut self) {
        self.reset()
    }

    fn current_decimal(&self) -> Option<Decimal> {
        self.current()
    }
}

/// Registry for managing multiple transforms.
///
/// Provides:
/// - Named registration and lookup of transforms
/// - Batch update of all transforms
/// - Maximum warmup calculation
/// - All-ready checking
///
/// # Example
///
/// ```ignore
/// use trading_common::transforms::{TransformRegistry, Sma, Rsi};
///
/// let mut registry = TransformRegistry::new();
/// registry.register(Sma::new(20));
/// registry.register(Sma::new(50));
/// registry.register(Rsi::new(14).sma(3));
///
/// // Update all transforms at once
/// registry.update(&bars);
///
/// // Check if all ready
/// if registry.all_ready(&bars) {
///     let sma20 = registry.get("sma_20").unwrap();
///     let rsi = registry.get("sma_3_of_rsi_14").unwrap();
/// }
/// ```
#[derive(Default)]
pub struct TransformRegistry {
    transforms: HashMap<String, Box<dyn TransformDyn>>,
    /// Insertion order for deterministic updates
    order: Vec<String>,
}

impl TransformRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a transform.
    ///
    /// The transform's name is used as the key.
    /// If a transform with the same name already exists, it is replaced.
    pub fn register<T: Transform<Output = Decimal> + 'static>(&mut self, transform: T) {
        let name = transform.name().to_string();
        if !self.transforms.contains_key(&name) {
            self.order.push(name.clone());
        }
        self.transforms.insert(name, Box::new(transform));
    }

    /// Register a transform with a custom name.
    ///
    /// Useful when you want to use a different name than the default.
    pub fn register_as<T: Transform<Output = Decimal> + 'static>(
        &mut self,
        name: &str,
        transform: T,
    ) {
        let name = name.to_string();
        if !self.transforms.contains_key(&name) {
            self.order.push(name.clone());
        }
        self.transforms.insert(name, Box::new(transform));
    }

    /// Update all registered transforms with new bar data.
    ///
    /// Transforms are updated in registration order.
    pub fn update(&mut self, bars: &BarsContext) {
        for name in &self.order {
            if let Some(transform) = self.transforms.get_mut(name) {
                transform.compute_decimal(bars);
            }
        }
    }

    /// Get the current value of a transform by name.
    pub fn get(&self, name: &str) -> Option<Decimal> {
        self.transforms.get(name)?.current_decimal()
    }

    /// Check if a transform exists in the registry.
    pub fn contains(&self, name: &str) -> bool {
        self.transforms.contains_key(name)
    }

    /// Check if all registered transforms are ready.
    pub fn all_ready(&self, bars: &BarsContext) -> bool {
        self.transforms.values().all(|t| t.is_ready_dyn(bars))
    }

    /// Get the maximum warmup period across all transforms.
    ///
    /// Useful for determining how many bars to skip in a backtest.
    pub fn max_warmup(&self) -> usize {
        self.transforms
            .values()
            .map(|t| t.warmup_period_dyn())
            .max()
            .unwrap_or(0)
    }

    /// Get the number of registered transforms.
    pub fn len(&self) -> usize {
        self.transforms.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.transforms.is_empty()
    }

    /// Get the names of all registered transforms.
    pub fn names(&self) -> Vec<&str> {
        self.order.iter().map(|s| s.as_str()).collect()
    }

    /// Reset all transforms (for new backtest run).
    pub fn reset(&mut self) {
        for transform in self.transforms.values_mut() {
            transform.reset_dyn();
        }
    }

    /// Remove a transform by name.
    ///
    /// Returns true if the transform was removed.
    pub fn remove(&mut self, name: &str) -> bool {
        if self.transforms.remove(name).is_some() {
            self.order.retain(|n| n != name);
            true
        } else {
            false
        }
    }

    /// Clear all transforms from the registry.
    pub fn clear(&mut self) {
        self.transforms.clear();
        self.order.clear();
    }
}

impl std::fmt::Debug for TransformRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransformRegistry")
            .field("transforms", &self.order)
            .field("count", &self.transforms.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{BarData, OHLCData, Timeframe};
    use crate::transforms::{Ema, Rsi, Sma, TransformExt};
    use chrono::Utc;
    use rust_decimal_macros::dec;

    fn create_bars_with_closes(closes: &[Decimal]) -> BarsContext {
        let mut ctx = BarsContext::new("TEST");
        for close in closes {
            let ohlc = OHLCData::new(
                Utc::now(),
                "TEST".to_string(),
                Timeframe::OneMinute,
                *close,
                *close,
                *close,
                *close,
                dec!(100),
                1,
            );
            ctx.on_bar_update(&BarData::from_ohlc(&ohlc));
        }
        ctx
    }

    #[test]
    fn test_registry_new() {
        let registry = TransformRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_registry_register() {
        let mut registry = TransformRegistry::new();

        registry.register(Sma::new(20));
        registry.register(Ema::new(10));

        assert_eq!(registry.len(), 2);
        assert!(registry.contains("sma_20"));
        assert!(registry.contains("ema_10"));
    }

    #[test]
    fn test_registry_register_as() {
        let mut registry = TransformRegistry::new();

        registry.register_as("fast_ma", Sma::new(10));
        registry.register_as("slow_ma", Sma::new(50));

        assert!(registry.contains("fast_ma"));
        assert!(registry.contains("slow_ma"));
        assert!(!registry.contains("sma_10"));
    }

    #[test]
    fn test_registry_update_and_get() {
        let mut registry = TransformRegistry::new();
        registry.register(Sma::new(3));

        let closes = vec![dec!(10), dec!(20), dec!(30)];
        let ctx = create_bars_with_closes(&closes);

        registry.update(&ctx);

        // SMA(3) of [10, 20, 30] = 20
        let value = registry.get("sma_3");
        assert_eq!(value, Some(dec!(20)));
    }

    #[test]
    fn test_registry_max_warmup() {
        let mut registry = TransformRegistry::new();

        registry.register(Sma::new(5)); // warmup = 5
        registry.register(Sma::new(10)); // warmup = 10
        registry.register(Rsi::new(14)); // warmup = 15

        assert_eq!(registry.max_warmup(), 15);
    }

    #[test]
    fn test_registry_all_ready() {
        let mut registry = TransformRegistry::new();

        registry.register(Sma::new(3));
        registry.register(Sma::new(5));

        let ctx_small = create_bars_with_closes(&[dec!(10), dec!(20), dec!(30)]);
        assert!(!registry.all_ready(&ctx_small)); // SMA(5) needs 5 bars

        let ctx_enough =
            create_bars_with_closes(&[dec!(10), dec!(20), dec!(30), dec!(40), dec!(50)]);
        assert!(registry.all_ready(&ctx_enough));
    }

    #[test]
    fn test_registry_names() {
        let mut registry = TransformRegistry::new();

        registry.register(Sma::new(20));
        registry.register(Ema::new(10));
        registry.register(Rsi::new(14));

        let names = registry.names();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"sma_20"));
        assert!(names.contains(&"ema_10"));
        assert!(names.contains(&"rsi_14"));
    }

    #[test]
    fn test_registry_reset() {
        let mut registry = TransformRegistry::new();
        registry.register(Sma::new(3));

        let closes = vec![dec!(10), dec!(20), dec!(30)];
        let ctx = create_bars_with_closes(&closes);

        registry.update(&ctx);
        assert!(registry.get("sma_3").is_some());

        registry.reset();
        assert!(registry.get("sma_3").is_none());
    }

    #[test]
    fn test_registry_remove() {
        let mut registry = TransformRegistry::new();

        registry.register(Sma::new(20));
        registry.register(Sma::new(50));

        assert_eq!(registry.len(), 2);

        let removed = registry.remove("sma_20");
        assert!(removed);
        assert_eq!(registry.len(), 1);
        assert!(!registry.contains("sma_20"));
        assert!(registry.contains("sma_50"));

        // Removing non-existent returns false
        let not_removed = registry.remove("sma_20");
        assert!(!not_removed);
    }

    #[test]
    fn test_registry_clear() {
        let mut registry = TransformRegistry::new();

        registry.register(Sma::new(20));
        registry.register(Sma::new(50));

        assert_eq!(registry.len(), 2);

        registry.clear();

        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_registry_with_chained_transforms() {
        let mut registry = TransformRegistry::new();

        // Register chained transforms
        registry.register(Rsi::new(5).sma(3));
        registry.register(Sma::new(3).ema(2));

        assert!(registry.contains("sma_3_of_rsi_5"));
        assert!(registry.contains("ema_2_of_sma_3"));

        // Max warmup should be 6 + 3 = 9 (RSI.SMA)
        assert_eq!(registry.max_warmup(), 9);
    }

    #[test]
    fn test_registry_update_order() {
        let mut registry = TransformRegistry::new();

        // Register in specific order
        registry.register(Sma::new(3));
        registry.register(Ema::new(3));
        registry.register(Rsi::new(5));

        // Names should be in insertion order
        let names = registry.names();
        assert_eq!(names[0], "sma_3");
        assert_eq!(names[1], "ema_3");
        assert_eq!(names[2], "rsi_5");
    }

    #[test]
    fn test_registry_replace_transform() {
        let mut registry = TransformRegistry::new();

        registry.register(Sma::new(10));

        let closes = vec![
            dec!(10),
            dec!(20),
            dec!(30),
            dec!(40),
            dec!(50),
            dec!(60),
            dec!(70),
            dec!(80),
            dec!(90),
            dec!(100),
        ];
        let ctx = create_bars_with_closes(&closes);
        registry.update(&ctx);

        let val1 = registry.get("sma_10").unwrap();

        // Register same name - should replace
        registry.register(Sma::new(10));

        // After replacing, value should be None until update
        assert!(registry.get("sma_10").is_none());

        // Update again
        registry.update(&ctx);
        let val2 = registry.get("sma_10").unwrap();

        // Values should be the same (same calculation)
        assert_eq!(val1, val2);
    }

    #[test]
    fn test_registry_get_nonexistent() {
        let registry = TransformRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_debug() {
        let mut registry = TransformRegistry::new();
        registry.register(Sma::new(20));

        let debug_str = format!("{:?}", registry);
        assert!(debug_str.contains("TransformRegistry"));
        assert!(debug_str.contains("sma_20"));
    }
}
