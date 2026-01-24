//! Mock data provider for testing
//!
//! Provides a simple mock implementation of the provider traits
//! for use in tests and development.

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use tokio::sync::broadcast;

use crate::provider::{
    ConnectionStatus, DataAvailability, DataProvider, DataType, HistoricalDataProvider,
    HistoricalRequest, LiveStreamProvider, LiveSubscription, ProviderError, ProviderInfo,
    ProviderResult, StreamCallback, StreamEvent, SubscriptionStatus,
};
use crate::schema::NormalizedOHLC;
use crate::symbol::SymbolSpec;
use trading_common::data::types::{TickData, TradeSide};

/// Mock data provider for testing
pub struct MockProvider {
    info: ProviderInfo,
    connected: bool,
    subscription_status: SubscriptionStatus,
    /// Number of ticks to generate for historical requests
    pub ticks_per_symbol: usize,
    /// Base price for generated data
    pub base_price: Decimal,
    /// Price volatility (random variation)
    pub price_volatility: Decimal,
}

impl MockProvider {
    /// Create a new mock provider
    pub fn new() -> Self {
        Self {
            info: ProviderInfo {
                name: "mock".to_string(),
                display_name: "Mock Provider".to_string(),
                version: "1.0.0".to_string(),
                supported_exchanges: vec!["MOCK".to_string()],
                supported_data_types: vec![DataType::Trades, DataType::OHLC],
                supports_historical: true,
                supports_live: true,
            },
            connected: false,
            subscription_status: SubscriptionStatus {
                symbols: vec![],
                connection: ConnectionStatus::Disconnected,
                messages_received: 0,
                last_message: None,
            },
            ticks_per_symbol: 1000,
            base_price: Decimal::from(100),
            price_volatility: Decimal::from(1),
        }
    }

    /// Generate mock tick data
    fn generate_ticks(
        &self,
        symbol: &SymbolSpec,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        limit: Option<usize>,
    ) -> Vec<TickData> {
        let duration = end - start;
        let num_ticks = limit
            .unwrap_or(self.ticks_per_symbol)
            .min(self.ticks_per_symbol);
        let interval = duration / num_ticks as i32;

        let mut ticks = Vec::with_capacity(num_ticks);
        let mut current_price = self.base_price;

        for i in 0..num_ticks {
            let timestamp = start + interval * i as i32;

            // Simple random walk for price
            let delta = if i % 2 == 0 {
                self.price_volatility
            } else {
                -self.price_volatility
            };
            current_price = current_price + delta;

            let tick = TickData::with_details(
                timestamp,
                timestamp,
                symbol.symbol.clone(),
                symbol.exchange.clone(),
                current_price,
                Decimal::from(i % 100 + 1), // size 1-100
                if i % 2 == 0 {
                    TradeSide::Buy
                } else {
                    TradeSide::Sell
                },
                "mock".to_string(),
                format!("mock_trade_{}", i),
                i % 2 == 0,
                i as i64,
            );
            ticks.push(tick);
        }

        ticks
    }
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DataProvider for MockProvider {
    fn info(&self) -> &ProviderInfo {
        &self.info
    }

    async fn connect(&mut self) -> ProviderResult<()> {
        self.connected = true;
        self.subscription_status.connection = ConnectionStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ProviderResult<()> {
        self.connected = false;
        self.subscription_status.connection = ConnectionStatus::Disconnected;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    async fn discover_symbols(&self, _exchange: Option<&str>) -> ProviderResult<Vec<SymbolSpec>> {
        if !self.connected {
            return Err(ProviderError::NotConnected);
        }

        Ok(vec![
            SymbolSpec::new("MOCK1", "MOCK"),
            SymbolSpec::new("MOCK2", "MOCK"),
            SymbolSpec::new("MOCK3", "MOCK"),
        ])
    }
}

#[async_trait]
impl HistoricalDataProvider for MockProvider {
    async fn fetch_ticks(
        &self,
        request: &HistoricalRequest,
    ) -> ProviderResult<Box<dyn Iterator<Item = ProviderResult<TickData>> + Send>> {
        if !self.connected {
            return Err(ProviderError::NotConnected);
        }

        let mut all_ticks = Vec::new();
        for symbol in &request.symbols {
            let ticks = self.generate_ticks(symbol, request.start, request.end, request.limit);
            all_ticks.extend(ticks);
        }

        // Sort by timestamp
        all_ticks.sort_by_key(|t| t.timestamp);

        Ok(Box::new(all_ticks.into_iter().map(Ok)))
    }

    async fn fetch_ohlc(&self, request: &HistoricalRequest) -> ProviderResult<Vec<NormalizedOHLC>> {
        if !self.connected {
            return Err(ProviderError::NotConnected);
        }

        // Generate OHLC from ticks
        let mut ohlc_bars = Vec::new();
        for symbol in &request.symbols {
            let ticks = self.generate_ticks(symbol, request.start, request.end, Some(100));

            // Group into 1-minute bars
            let mut current_bar_start = request.start;
            let bar_duration = Duration::minutes(1);
            let mut bar_ticks = Vec::new();

            for tick in ticks {
                if tick.timestamp >= current_bar_start + bar_duration {
                    if !bar_ticks.is_empty() {
                        let ohlc = create_ohlc_from_ticks(&bar_ticks, current_bar_start);
                        ohlc_bars.push(ohlc);
                    }
                    current_bar_start = tick.timestamp;
                    bar_ticks.clear();
                }
                bar_ticks.push(tick);
            }

            // Add final bar
            if !bar_ticks.is_empty() {
                let ohlc = create_ohlc_from_ticks(&bar_ticks, current_bar_start);
                ohlc_bars.push(ohlc);
            }
        }

        Ok(ohlc_bars)
    }

    async fn check_availability(
        &self,
        _symbol: &SymbolSpec,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> ProviderResult<DataAvailability> {
        if !self.connected {
            return Err(ProviderError::NotConnected);
        }

        Ok(DataAvailability {
            available: true,
            actual_start: Some(start),
            actual_end: Some(end),
            estimated_records: Some(self.ticks_per_symbol as u64),
        })
    }
}

#[async_trait]
impl LiveStreamProvider for MockProvider {
    async fn subscribe(
        &mut self,
        subscription: LiveSubscription,
        callback: StreamCallback,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> ProviderResult<()> {
        if !self.connected {
            return Err(ProviderError::NotConnected);
        }

        self.subscription_status.symbols = subscription.symbols.clone();
        callback(StreamEvent::Status(ConnectionStatus::Connected));

        // Generate mock data at regular intervals
        let mut sequence = 0i64;
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(100));

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    break;
                }
                _ = interval.tick() => {
                    for symbol in &subscription.symbols {
                        let now = Utc::now();
                        let tick = TickData::with_details(
                            now,
                            now,
                            symbol.symbol.clone(),
                            symbol.exchange.clone(),
                            self.base_price,
                            Decimal::from(10),
                            if sequence % 2 == 0 { TradeSide::Buy } else { TradeSide::Sell },
                            "mock".to_string(),
                            format!("mock_{}", sequence),
                            sequence % 2 == 0,
                            sequence,
                        );
                        callback(StreamEvent::Tick(tick));
                        self.subscription_status.messages_received += 1;
                        self.subscription_status.last_message = Some(now);
                        sequence += 1;
                    }
                }
            }
        }

        callback(StreamEvent::Status(ConnectionStatus::Disconnected));
        Ok(())
    }

    async fn unsubscribe(&mut self, symbols: &[SymbolSpec]) -> ProviderResult<()> {
        self.subscription_status
            .symbols
            .retain(|s| !symbols.contains(s));
        Ok(())
    }

    fn subscription_status(&self) -> SubscriptionStatus {
        self.subscription_status.clone()
    }
}

fn create_ohlc_from_ticks(ticks: &[TickData], timestamp: DateTime<Utc>) -> NormalizedOHLC {
    let open = ticks.first().map(|t| t.price).unwrap_or(Decimal::ZERO);
    let close = ticks.last().map(|t| t.price).unwrap_or(Decimal::ZERO);
    let high = ticks.iter().map(|t| t.price).max().unwrap_or(Decimal::ZERO);
    let low = ticks.iter().map(|t| t.price).min().unwrap_or(Decimal::ZERO);
    let volume = ticks.iter().map(|t| t.quantity).sum();

    NormalizedOHLC::new(
        timestamp,
        ticks.first().map(|t| t.symbol.clone()).unwrap_or_default(),
        ticks
            .first()
            .map(|t| t.exchange.clone())
            .unwrap_or_default(),
        "1m".to_string(),
        open,
        high,
        low,
        close,
        volume,
        ticks.len() as u64,
        "mock".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_provider_connect() {
        let mut provider = MockProvider::new();
        assert!(!provider.is_connected());

        provider.connect().await.unwrap();
        assert!(provider.is_connected());

        provider.disconnect().await.unwrap();
        assert!(!provider.is_connected());
    }

    #[tokio::test]
    async fn test_mock_provider_fetch_ticks() {
        let mut provider = MockProvider::new();
        provider.ticks_per_symbol = 100;
        provider.connect().await.unwrap();

        let request = HistoricalRequest::trades(
            vec![SymbolSpec::new("TEST", "MOCK")],
            Utc::now() - Duration::hours(1),
            Utc::now(),
        );

        let ticks: Vec<_> = provider.fetch_ticks(&request).await.unwrap().collect();

        assert_eq!(ticks.len(), 100);
        assert!(ticks.iter().all(|t| t.is_ok()));
    }

    #[tokio::test]
    async fn test_mock_provider_discover_symbols() {
        let mut provider = MockProvider::new();
        provider.connect().await.unwrap();

        let symbols = provider.discover_symbols(None).await.unwrap();
        assert!(!symbols.is_empty());
    }
}
