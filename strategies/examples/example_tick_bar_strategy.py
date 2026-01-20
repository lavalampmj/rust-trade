"""
Example Tick Bar Strategy using N-Tick OHLC Bars

This strategy demonstrates:
- Working with N-tick bars (e.g., 100-tick bars)
- Volume-weighted price analysis using BarsContext
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
Updated to use BarsContext for NinjaTrader-style access.
"""

from typing import Optional, Dict, Any
from decimal import Decimal
from _lib.base_strategy import BaseStrategy, Signal
from _lib.bars_context import BarsContext


class TickBarStrategy(BaseStrategy):
    """
    N-Tick bar strategy that analyzes price action over fixed tick counts.

    Unlike time-based bars where each bar represents a fixed time period,
    tick bars represent a fixed number of trades. This gives you:
    - Consistent statistical properties across different market conditions
    - More bars during high activity, fewer during low activity
    - Better representation of actual market flow

    Uses BarsContext for volume tracking and price series.
    """

    def __init__(self):
        """Initialize the tick bar strategy."""
        # Strategy configuration
        self.expected_tick_count = 100  # Expected ticks per bar
        self.lookback_bars = 20  # Number of bars to analyze

        # Price action thresholds
        self.momentum_threshold = Decimal("0.002")  # 0.2% price move threshold
        self.volume_multiplier = Decimal("1.5")  # Volume must be 1.5x average

        # Position tracking
        self.has_position = False
        self.entry_price = None

        # Statistics
        self.total_bars_processed = 0
        self.avg_bar_volume = None

    def name(self) -> str:
        """Return the strategy name."""
        return "N-Tick Bar Momentum Strategy (Python)"

    def is_ready(self, bars: BarsContext) -> bool:
        """Ready when we have enough bars for lookback analysis."""
        return bars.is_ready_for(self.lookback_bars)

    def warmup_period(self) -> int:
        """Return lookback_bars as warmup requirement."""
        return self.lookback_bars

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

            if "momentum_threshold" in params:
                self.momentum_threshold = Decimal(params["momentum_threshold"])
                if self.momentum_threshold <= 0:
                    return "momentum_threshold must be positive"

            if "volume_multiplier" in params:
                self.volume_multiplier = Decimal(params["volume_multiplier"])
                if self.volume_multiplier <= 0:
                    return "volume_multiplier must be positive"

            return None

        except ValueError as e:
            return str(e)

    def reset(self) -> None:
        """Reset strategy state for new backtest."""
        self.has_position = False
        self.entry_price = None
        self.total_bars_processed = 0
        self.avg_bar_volume = None

    def on_bar_data(self, bar_data: Dict[str, Any], bars: BarsContext) -> Dict[str, Any]:
        """
        Process N-tick bar and generate trading signal.

        Uses BarsContext for price and volume series access.

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
            bars: BarsContext with OHLCV series and helpers

        Returns:
            Signal dictionary
        """
        symbol = bar_data["symbol"]

        self.total_bars_processed += 1

        # Use BarsContext for current bar data
        if bars.open.is_empty():
            return Signal.hold()

        open_price = bars.open[0]
        close_price = bars.close[0]
        volume = bars.volume[0]

        # Calculate momentum
        if open_price == 0:
            return Signal.hold()

        momentum = (close_price - open_price) / open_price

        # Update average volume
        if self.avg_bar_volume is None:
            self.avg_bar_volume = volume
        else:
            # Exponential moving average with alpha=0.2
            alpha = Decimal("0.2")
            self.avg_bar_volume = alpha * volume + (1 - alpha) * self.avg_bar_volume

        # Defense-in-depth: early return if not ready
        # (Engine also checks, but strategy can check for explicit handling)
        if not self.is_ready(bars):
            return Signal.hold()

        # Check for high volume
        is_high_volume = volume >= (self.avg_bar_volume * self.volume_multiplier)

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
