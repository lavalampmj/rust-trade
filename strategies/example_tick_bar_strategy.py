"""
Example Tick Bar Strategy using N-Tick OHLC Bars

This strategy demonstrates:
- Working with N-tick bars (e.g., 100-tick bars)
- Volume-weighted price analysis
- Tick intensity monitoring
- Position sizing based on bar characteristics

Strategy Logic:
- Uses 100-tick bars (100 trades per bar)
- Monitors volume and price action within each bar
- Buys on high volume bars with upward price movement
- Sells on high volume bars with downward price movement

Key Concepts:
- trade_count field indicates number of ticks in the bar
- For 100-tick bars, trade_count should be 100 (except last bar)
- Volume represents total traded volume in those 100 ticks
- Bar duration varies based on market activity

Uses the unified on_bar_data() interface with OnCloseBar mode and TickBased bars.
Updated to use Series<T> abstraction for NinjaTrader-style access.
"""

from typing import Optional, Dict, Any
from decimal import Decimal
from base_strategy import BaseStrategy, Signal, FloatSeries, Series


class TickBarStrategy(BaseStrategy):
    """
    N-Tick bar strategy that analyzes price action over fixed tick counts.

    Unlike time-based bars where each bar represents a fixed time period,
    tick bars represent a fixed number of trades. This gives you:
    - Consistent statistical properties across different market conditions
    - More bars during high activity, fewer during low activity
    - Better representation of actual market flow

    Uses FloatSeries for volume tracking and Series for bar history.
    """

    def __init__(self):
        """Initialize the tick bar strategy."""
        # Strategy configuration
        self.expected_tick_count = 100  # Expected ticks per bar
        self.lookback_bars = 20  # Number of bars to analyze

        # Price action thresholds
        self.momentum_threshold = 0.002  # 0.2% price move threshold
        self.volume_multiplier = 1.5  # Volume must be 1.5x average

        # Use FloatSeries for volume and momentum tracking
        self.volumes = FloatSeries("volumes", max_lookback=self.lookback_bars)
        self.momentums = FloatSeries("momentums", max_lookback=self.lookback_bars)
        self.closes = FloatSeries("closes", max_lookback=self.lookback_bars)

        # Position tracking
        self.has_position = False
        self.entry_price = None

        # Statistics
        self.total_bars_processed = 0
        self.avg_bar_volume = None

    def name(self) -> str:
        """Return the strategy name."""
        return "N-Tick Bar Momentum Strategy (Python)"

    def initialize(self, params: Dict[str, str]) -> Optional[str]:
        """
        Initialize strategy with parameters.

        Supported parameters:
        - tick_count: Expected ticks per bar (default: 100)
        - lookback_bars: Bars to analyze for statistics (default: 20)
        - momentum_threshold: Min price move % to trigger (default: 0.002)
        - volume_multiplier: Volume threshold multiplier (default: 1.5)

        Returns:
            None if successful, error message if failed
        """
        try:
            if "tick_count" in params:
                self.expected_tick_count = int(params["tick_count"])
                if self.expected_tick_count < 10:
                    return "tick_count must be at least 10"

            if "lookback_bars" in params:
                self.lookback_bars = int(params["lookback_bars"])
                if self.lookback_bars < 2:
                    return "lookback_bars must be at least 2"
                # Reinitialize series with new lookback
                self.volumes = FloatSeries("volumes", max_lookback=self.lookback_bars)
                self.momentums = FloatSeries("momentums", max_lookback=self.lookback_bars)
                self.closes = FloatSeries("closes", max_lookback=self.lookback_bars)

            if "momentum_threshold" in params:
                self.momentum_threshold = float(params["momentum_threshold"])
                if self.momentum_threshold <= 0:
                    return "momentum_threshold must be positive"

            if "volume_multiplier" in params:
                self.volume_multiplier = float(params["volume_multiplier"])
                if self.volume_multiplier <= 0:
                    return "volume_multiplier must be positive"

            return None

        except ValueError as e:
            return str(e)

    def reset(self) -> None:
        """Reset strategy state for new backtest."""
        self.volumes.reset()
        self.momentums.reset()
        self.closes.reset()
        self.has_position = False
        self.entry_price = None
        self.total_bars_processed = 0
        self.avg_bar_volume = None

    def calculate_bar_momentum(self, bar_data: Dict[str, Any]) -> float:
        """
        Calculate momentum for a single bar.

        Momentum = (close - open) / open

        Args:
            bar_data: Bar data dictionary

        Returns:
            Momentum as decimal (e.g., 0.002 = 0.2% upward movement)
        """
        open_price = float(bar_data["open"])
        close_price = float(bar_data["close"])

        if open_price == 0:
            return 0.0

        return (close_price - open_price) / open_price

    def update_volume_stats(self, volume: float) -> None:
        """
        Update rolling average volume statistics.

        Args:
            volume: Current bar volume
        """
        if self.avg_bar_volume is None:
            self.avg_bar_volume = volume
        else:
            # Exponential moving average with alpha=0.2
            alpha = 0.2
            self.avg_bar_volume = alpha * volume + (1 - alpha) * self.avg_bar_volume

    def is_high_volume_bar(self, volume: float) -> bool:
        """
        Check if current bar has high volume.

        Args:
            volume: Current bar volume

        Returns:
            True if volume exceeds threshold
        """
        if self.avg_bar_volume is None or self.avg_bar_volume == 0:
            return False

        return volume >= (self.avg_bar_volume * self.volume_multiplier)

    def on_bar_data(self, bar_data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Process N-tick bar and generate trading signal.

        This is where the magic happens! Each bar represents exactly N trades
        (configured via tick_count parameter).

        Args:
            bar_data: Dictionary with keys:
                - timestamp: Bar start time (first tick's timestamp)
                - symbol: Trading pair
                - open: First tick's price in this bar
                - high: Highest price among N ticks
                - low: Lowest price among N ticks
                - close: Last tick's price in this bar
                - volume: Total volume of N ticks
                - trade_count: Number of ticks (should be N, or less for last bar)
                - is_bar_closed: Whether this bar is complete
                - tick_count_in_bar: Number of ticks in this bar

        Returns:
            Signal dictionary
        """
        symbol = bar_data["symbol"]
        volume = float(bar_data["volume"])
        close_price = float(bar_data["close"])

        self.total_bars_processed += 1

        # Update statistics
        self.update_volume_stats(volume)

        # Calculate bar characteristics
        momentum = self.calculate_bar_momentum(bar_data)
        is_high_volume = self.is_high_volume_bar(volume)

        # Push to series (NinjaTrader-style)
        self.volumes.push(volume)
        self.momentums.push(momentum)
        self.closes.push(close_price)

        # Need enough history before trading
        if self.volumes.count() < self.lookback_bars:
            return Signal.hold()

        # Trading Logic:
        # Buy: Strong upward momentum + high volume + no position
        if (not self.has_position and
            momentum > self.momentum_threshold and
            is_high_volume):

            self.has_position = True
            self.entry_price = close_price

            # Position size: 10% of capital
            return Signal.buy(symbol, "0.1")

        # Sell: Strong downward momentum + high volume + have position
        if (self.has_position and
            momentum < -self.momentum_threshold and
            is_high_volume):

            self.has_position = False
            self.entry_price = None

            return Signal.sell(symbol, "0.1")

        # Also sell if we have a position and momentum reverses strongly
        if (self.has_position and
            momentum < 0 and
            abs(momentum) > self.momentum_threshold / 2):

            self.has_position = False
            self.entry_price = None

            return Signal.sell(symbol, "0.1")

        return Signal.hold()

    def bar_data_mode(self) -> str:
        """Return OnCloseBar mode - process only completed bars."""
        return "OnCloseBar"

    def preferred_bar_type(self) -> Dict[str, Any]:
        """Prefer 100-tick bars for this strategy."""
        return {"type": "TickBased", "tick_count": self.expected_tick_count}


# Example usage in Rust:
"""
To use this strategy with 100-tick bars in your Rust backtest:

```rust
use trading_common::data::repository::TickDataRepository;
use chrono::{Utc, Duration};

// Generate 100-tick bars instead of time-based bars
let tick_bars = repository
    .generate_n_tick_ohlc(
        "BTCUSDT",
        100,  // 100 ticks per bar
        start_time,
        end_time,
        None
    )
    .await?;

// Load the Python strategy
let mut strategy = PythonStrategy::from_file(
    "strategies/example_tick_bar_strategy.py",
    "TickBarStrategy"
)?;

// Initialize with parameters
let mut params = HashMap::new();
params.insert("tick_count".to_string(), "100".to_string());
params.insert("lookback_bars".to_string(), "20".to_string());
strategy.initialize(params)?;

// Feed tick bars to strategy using unified interface
for bar in tick_bars {
    let bar_data = BarData::from_ohlc(&bar);
    let signal = strategy.on_bar_data(&bar_data);
    // Process signal...
}
```

Key Advantages of N-Tick Bars:
1. Consistent statistical properties (always same number of ticks)
2. Activity-based sampling (more bars when market is active)
3. Better for high-frequency strategies
4. Eliminates time-based bias
5. Works well with volume-based indicators

Common N-Tick Bar Sizes:
- 50 ticks: Very high frequency, lots of bars
- 100 ticks: High frequency, good for scalping
- 500 ticks: Medium frequency, good for day trading
- 1000 ticks: Lower frequency, good for swing trading
"""
