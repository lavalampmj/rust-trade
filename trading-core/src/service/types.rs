/// Processing statistics for the market data service
#[derive(Debug, Clone, Default)]
pub struct ProcessingStats {
    /// Total ticks processed
    pub total_ticks_processed: u64,
    /// Cache update failures
    pub cache_update_failures: u64,
}
