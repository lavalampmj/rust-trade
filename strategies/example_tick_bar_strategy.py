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
"""

from typing import Optional, Dict, Any
from collections import deque
from decimal import Decimal
from base_strategy import BaseStrategy, Signal


class TickBarStrategy(BaseStrategy):
    """
    N-Tick bar strategy that analyzes price action over fixed tick counts.

    Unlike time-based bars where each bar represents a fixed time period,
    tick bars represent a fixed number of trades. This gives you:
    - Consistent statistical properties across different market conditions
    - More bars during high activity, fewer during low activity
    - Better representation of actual market flow
    """

    def __init__(self):
        """Initialize the tick bar strategy."""
        # Strategy configuration
        self.expected_tick_count = 100  # Expected ticks per bar
        self.lookback_bars = 20  # Number of bars to analyze

        # Price action thresholds
        self.momentum_threshold = 0.002  # 0.2% price move threshold
        self.volume_multiplier = 1.5  # Volume must be 1.5x average

        # Bar history for analysis
        self.bar_history = deque(maxlen=self.lookback_bars)

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
                self.bar_history = deque(maxlen=self.lookback_bars)

            if "momentum_threshold" in params:
                self.momentum_threshold = float(params["momentum_threshold"])
                if self.momentum_threshold <= 0:
                    return "momentum_threshold must be positive"

            if "volume_multiplier" in params:
                self.volume_multiplier = float(params["volume_multiplier"])
                if self.volume_multiplier <= 0:
                    return "volume_multiplier must be positive"

            print(f"TickBar Strategy initialized:")
            print(f"  Expected ticks/bar: {self.expected_tick_count}")
            print(f"  Lookback period: {self.lookback_bars} bars")
            print(f"  Momentum threshold: {self.momentum_threshold*100:.2f}%")
            print(f"  Volume multiplier: {self.volume_multiplier}x")

            return None

        except ValueError as e:
            return f"Parameter error: {str(e)}"

    def reset(self) -> None:
        """Reset strategy state for new backtest."""
        self.bar_history.clear()
        self.has_position = False
        self.entry_price = None
        self.total_bars_processed = 0
        self.avg_bar_volume = None

    def calculate_bar_momentum(self, bar: Dict[str, Any]) -> float:
        """
        Calculate momentum for a single bar.

        Momentum = (close - open) / open

        Args:
            bar: OHLC bar dictionary

        Returns:
            Momentum as decimal (e.g., 0.002 = 0.2% upward movement)
        """
        open_price = float(bar["open"])
        close_price = float(bar["close"])

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

    def on_tick(self, tick: Dict[str, Any]) -> Dict[str, Any]:
        """
        Not used - this strategy only works with OHLC bars.

        Args:
            tick: Tick data dictionary

        Returns:
            Hold signal
        """
        return Signal.hold()

    def on_ohlc(self, ohlc: Dict[str, Any]) -> Dict[str, Any]:
        """
        Process N-tick bar and generate trading signal.

        This is where the magic happens! Each bar represents exactly N trades
        (configured via tick_count parameter).

        Args:
            ohlc: Dictionary with keys:
                - timestamp: Bar start time (first tick's timestamp)
                - symbol: Trading pair
                - timeframe: Placeholder (not meaningful for tick bars)
                - open: First tick's price in this bar
                - high: Highest price among N ticks
                - low: Lowest price among N ticks
                - close: Last tick's price in this bar
                - volume: Total volume of N ticks
                - trade_count: Number of ticks (should be N, or less for last bar)

        Returns:
            Signal dictionary
        """
        symbol = ohlc["symbol"]
        trade_count = ohlc["trade_count"]
        volume = float(ohlc["volume"])

        # Verify we're getting tick bars (not time-based bars)
        # For 100-tick bars, trade_count should consistently be 100
        if self.total_bars_processed < 5:
            print(f"Bar {self.total_bars_processed + 1}: "
                  f"trade_count={trade_count}, "
                  f"expected={self.expected_tick_count}")

        self.total_bars_processed += 1

        # Update statistics
        self.update_volume_stats(volume)

        # Calculate bar characteristics
        momentum = self.calculate_bar_momentum(ohlc)
        is_high_volume = self.is_high_volume_bar(volume)

        # Store bar in history for future analysis
        self.bar_history.append({
            "momentum": momentum,
            "volume": volume,
            "high": float(ohlc["high"]),
            "low": float(ohlc["low"]),
            "close": float(ohlc["close"]),
            "trade_count": trade_count,
        })

        # Need enough history before trading
        if len(self.bar_history) < self.lookback_bars:
            return Signal.hold()

        # Trading Logic:
        # Buy: Strong upward momentum + high volume + no position
        if (not self.has_position and
            momentum > self.momentum_threshold and
            is_high_volume):

            print(f"BUY Signal - Momentum: {momentum*100:.2f}%, "
                  f"Volume: {volume:.2f} (avg: {self.avg_bar_volume:.2f}), "
                  f"Ticks: {trade_count}")

            self.has_position = True
            self.entry_price = float(ohlc["close"])

            # Position size: 10% of capital
            return Signal.buy(symbol, "0.1")

        # Sell: Strong downward momentum + high volume + have position
        if (self.has_position and
            momentum < -self.momentum_threshold and
            is_high_volume):

            profit_pct = ((float(ohlc["close"]) - self.entry_price) /
                         self.entry_price * 100) if self.entry_price else 0

            print(f"SELL Signal - Momentum: {momentum*100:.2f}%, "
                  f"Volume: {volume:.2f}, "
                  f"P&L: {profit_pct:.2f}%")

            self.has_position = False
            self.entry_price = None

            return Signal.sell(symbol, "0.1")

        # Also sell if we have a position and momentum reverses strongly
        if (self.has_position and
            momentum < 0 and
            abs(momentum) > self.momentum_threshold / 2):

            profit_pct = ((float(ohlc["close"]) - self.entry_price) /
                         self.entry_price * 100) if self.entry_price else 0

            print(f"EXIT Signal - Momentum reversal: {momentum*100:.2f}%, "
                  f"P&L: {profit_pct:.2f}%")

            self.has_position = False
            self.entry_price = None

            return Signal.sell(symbol, "0.1")

        return Signal.hold()

    def supports_ohlc(self) -> bool:
        """Indicate that this strategy ONLY supports OHLC data."""
        return True

    def preferred_timeframe(self) -> Optional[str]:
        """
        Return preferred timeframe.

        Note: For tick bars, this is not used. The bar type is configured
        when you call repository.generate_n_tick_ohlc() from Rust.
        """
        return None


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

// Feed tick bars to strategy
for bar in tick_bars {
    let signal = strategy.on_ohlc(&bar);
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
