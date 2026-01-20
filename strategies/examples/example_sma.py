"""
Example Simple Moving Average (SMA) crossover strategy.

This strategy buys when the short-term moving average crosses above
the long-term moving average (golden cross) and sells when it crosses
below (death cross).

Uses the unified on_bar_data() interface with OnCloseBar mode,
processing only completed bars for efficiency.

Updated to use BarsContext for simplified price series access.
"""

from _lib.base_strategy import BaseStrategy, Signal
from _lib.bars_context import BarsContext
from typing import Dict, Any, Optional


class ExampleSmaStrategy(BaseStrategy):
    """
    Simple Moving Average crossover strategy using BarsContext.

    Generates buy signals on golden cross (short MA > long MA)
    and sell signals on death cross (short MA < long MA).

    Uses BarsContext for price history with built-in SMA calculation.
    """

    def __init__(self):
        """Initialize the SMA strategy with default parameters."""
        self.short_period = 5
        self.long_period = 20
        self.last_signal_type = None

    def name(self) -> str:
        """Return the strategy name."""
        return "Simple Moving Average (Python)"

    def is_ready(self, bars: BarsContext) -> bool:
        """Ready when we have enough data for the long SMA."""
        return bars.is_ready_for(self.long_period)

    def warmup_period(self) -> int:
        """Return the long period as warmup requirement."""
        return self.long_period

    def initialize(self, params: Dict[str, str]) -> Optional[str]:
        """
        Initialize strategy with parameters.

        Args:
            params: Dictionary with optional keys:
                - short_period: Number of periods for short MA (default: 5)
                - long_period: Number of periods for long MA (default: 20)

        Returns:
            None if successful, error message if failed
        """
        try:
            if "short_period" in params:
                self.short_period = int(params["short_period"])
            if "long_period" in params:
                self.long_period = int(params["long_period"])

            if self.short_period >= self.long_period:
                return "Short period must be less than long period"

            if self.short_period < 1 or self.long_period < 1:
                return "Periods must be positive integers"

            return None
        except Exception as e:
            return str(e)

    def on_bar_data(self, bar_data: Dict[str, Any], bars: BarsContext) -> Dict[str, Any]:
        """
        Process bar data and generate trading signal.

        Uses BarsContext for SMA calculation. No manual series management needed!

        Args:
            bar_data: Bar data dictionary with OHLC and metadata
            bars: BarsContext with OHLCV series and helpers

        Returns:
            Trading signal
        """
        # Defense-in-depth: early return if not ready
        # (Engine also checks, but strategy can check for explicit handling)
        if not self.is_ready(bars):
            return Signal.hold()

        symbol = bar_data["symbol"]

        # Use BarsContext's built-in SMA helpers - no manual series management needed!
        # Since we checked is_ready(), these are guaranteed to return values
        short_sma = bars.sma(self.short_period)
        long_sma = bars.sma(self.long_period)

        # Golden cross: short MA crosses above long MA
        if short_sma > long_sma and self.last_signal_type != "Buy":
            self.last_signal_type = "Buy"
            return Signal.buy(symbol, "100")

        # Death cross: short MA crosses below long MA
        elif short_sma < long_sma and self.last_signal_type == "Buy":
            self.last_signal_type = "Sell"
            return Signal.sell(symbol, "100")

        return Signal.hold()

    def reset(self):
        """Reset strategy state for new backtest."""
        self.last_signal_type = None

    def bar_data_mode(self) -> str:
        """Return OnCloseBar mode - process only completed bars."""
        return "OnCloseBar"

    def preferred_bar_type(self) -> Dict[str, Any]:
        """Prefer 1-minute time-based bars."""
        return {"type": "TimeBased", "timeframe": "1m"}
