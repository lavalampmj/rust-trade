"""
Example RSI (Relative Strength Index) Strategy in Python

This strategy demonstrates:
- RSI calculation using price changes
- Overbought/oversold threshold trading
- State management with BarsContext custom series
- Type hints for clarity

Strategy Logic:
- Buy when RSI < 30 (oversold)
- Sell when RSI > 70 (overbought)
- Hold otherwise

Uses the unified on_bar_data() interface with OnCloseBar mode.
Updated to use BarsContext for price change tracking.
"""

from typing import Optional, Dict, Any
from decimal import Decimal
from _lib.base_strategy import BaseStrategy, Signal
from _lib.bars_context import BarsContext


class ExampleRsiStrategy(BaseStrategy):
    """
    RSI strategy that buys when oversold (RSI < 30) and sells when overbought (RSI > 70).

    The RSI is calculated over a configurable period (default 14) using the
    average gains and losses.

    Uses BarsContext custom series for price change tracking.
    """

    def __init__(self):
        """Initialize the RSI strategy with default parameters."""
        self.period = 14  # RSI period
        self.oversold = Decimal("30")  # Buy threshold
        self.overbought = Decimal("70")  # Sell threshold

        # Track position state
        self.has_position = False

        # Series references (initialized on first call)
        self._gains_series = None
        self._losses_series = None

    def name(self) -> str:
        """Return the strategy name."""
        return "Relative Strength Index (Python)"

    def initialize(self, params: Dict[str, str]) -> Optional[str]:
        """
        Initialize strategy with parameters.

        Supported parameters:
        - period: RSI calculation period (default: 14)
        - oversold: Buy threshold (default: 30)
        - overbought: Sell threshold (default: 70)

        Returns error message if validation fails, None otherwise.
        """
        if "period" in params:
            try:
                period = int(params["period"])
                if period < 2:
                    return "RSI period must be at least 2"
                self.period = period
            except ValueError:
                return "Invalid period parameter: must be an integer"

        if "oversold" in params:
            try:
                oversold = Decimal(params["oversold"])
                if not (Decimal(0) <= oversold <= Decimal(100)):
                    return "Oversold threshold must be between 0 and 100"
                self.oversold = oversold
            except Exception:
                return "Invalid oversold parameter: must be a number"

        if "overbought" in params:
            try:
                overbought = Decimal(params["overbought"])
                if not (Decimal(0) <= overbought <= Decimal(100)):
                    return "Overbought threshold must be between 0 and 100"
                self.overbought = overbought
            except Exception:
                return "Invalid overbought parameter: must be a number"

        if self.oversold >= self.overbought:
            return "Oversold threshold must be less than overbought threshold"

        return None

    def reset(self) -> None:
        """Reset strategy state."""
        self.has_position = False
        self._gains_series = None
        self._losses_series = None

    def on_bar_data(self, bar_data: Dict[str, Any], bars: BarsContext) -> Dict[str, Any]:
        """
        Process bar data and generate trading signal.

        Uses BarsContext for price change calculation and custom series for
        tracking gains and losses.

        Args:
            bar_data: Bar data dictionary with OHLC and metadata
            bars: BarsContext with OHLCV series and helpers

        Returns:
            Signal dictionary (from Signal.buy/sell/hold)
        """
        symbol = bar_data["symbol"]

        # Register custom series on first call
        if not bars.has_series("gains"):
            self._gains_series = bars.register_series("gains")
            self._losses_series = bars.register_series("losses")

        # Calculate price change using bars
        change = bars.change()
        if change is None:
            bars.push_to_series("gains", Decimal(0))
            bars.push_to_series("losses", Decimal(0))
            return Signal.hold()

        # Track gains and losses
        gain = change if change > 0 else Decimal(0)
        loss = abs(change) if change < 0 else Decimal(0)
        bars.push_to_series("gains", gain)
        bars.push_to_series("losses", loss)

        # Need enough data
        if bars.count() < self.period:
            return Signal.hold()

        # Calculate RSI
        gains_series = bars.get_series("gains")
        losses_series = bars.get_series("losses")

        avg_gain = gains_series.sma(self.period)
        avg_loss = losses_series.sma(self.period)

        if avg_gain is None or avg_loss is None or avg_loss == 0:
            return Signal.hold()

        rs = avg_gain / avg_loss
        rsi = Decimal(100) - (Decimal(100) / (1 + rs))

        # Trading logic
        # Buy when oversold and don't have position
        if rsi < self.oversold and not self.has_position:
            self.has_position = True
            # Use 10% of capital for each trade
            return Signal.buy(symbol, "0.1")

        # Sell when overbought and have position
        if rsi > self.overbought and self.has_position:
            self.has_position = False
            return Signal.sell(symbol, "0.1")

        return Signal.hold()

    def bar_data_mode(self) -> str:
        """Return OnCloseBar mode - process only completed bars."""
        return "OnCloseBar"

    def preferred_bar_type(self) -> Dict[str, Any]:
        """Prefer 1-minute time-based bars."""
        return {"type": "TimeBased", "timeframe": "1m"}
