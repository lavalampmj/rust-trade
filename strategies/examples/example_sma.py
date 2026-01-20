"""
Example Simple Moving Average (SMA) crossover strategy.

This strategy buys when the short-term moving average crosses above
the long-term moving average (golden cross) and sells when it crosses
below (death cross).

Uses the unified on_bar_data() interface with OnCloseBar mode,
processing only completed bars for efficiency.

Updated to use Series<T> abstraction for NinjaTrader-style access
to historical price data.
"""

from _lib.base_strategy import BaseStrategy, Signal, DecimalSeries
from decimal import Decimal
from typing import Dict, Any, Optional


class ExampleSmaStrategy(BaseStrategy):
    """
    Simple Moving Average crossover strategy.

    Generates buy signals on golden cross (short MA > long MA)
    and sell signals on death cross (short MA < long MA).

    Uses DecimalSeries for price history with built-in SMA calculation.
    """

    def __init__(self):
        """Initialize the SMA strategy with default parameters."""
        self.short_period = 5
        self.long_period = 20
        # Use DecimalSeries for reverse-indexed access with SMA helper
        self.close = DecimalSeries("close", max_lookback=self.long_period * 2)
        self.last_signal_type = None

    def name(self) -> str:
        """Return the strategy name."""
        return "Simple Moving Average (Python)"

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

            # Reinitialize series with appropriate lookback
            self.close = DecimalSeries("close", max_lookback=self.long_period * 2)

            return None
        except Exception as e:
            return str(e)

    def on_bar_data(self, bar_data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Process bar data and generate trading signal.

        Uses close price for SMA calculation. Operates in OnCloseBar mode
        so only processes completed bars.

        Args:
            bar_data: Bar data dictionary with OHLC and metadata

        Returns:
            Trading signal
        """
        # Push close price to series (NinjaTrader-style)
        price = Decimal(bar_data["close"])
        symbol = bar_data["symbol"]
        self.close.push(price)

        # Use built-in SMA helpers from DecimalSeries
        short_sma = self.close.sma(self.short_period)
        long_sma = self.close.sma(self.long_period)

        # Need enough data for both SMAs
        if short_sma is None or long_sma is None:
            return Signal.hold()

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
        self.close.reset()
        self.last_signal_type = None

    def bar_data_mode(self) -> str:
        """Return OnCloseBar mode - process only completed bars."""
        return "OnCloseBar"

    def preferred_bar_type(self) -> Dict[str, Any]:
        """Prefer 1-minute time-based bars."""
        return {"type": "TimeBased", "timeframe": "1m"}
