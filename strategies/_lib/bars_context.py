"""
BarsContext - OHLCV series wrapper for Python strategies.

Provides synchronized access to price data with built-in indicator helpers,
matching the Rust BarsContext API exactly.
"""

from decimal import Decimal
from typing import Dict, Any, Optional, TypeVar, Generic
from .base_strategy import Series, DecimalSeries

T = TypeVar('T')


class BarsContext:
    """
    Wrapper providing synchronized OHLCV series access for strategies.

    BarsContext maintains OHLCV price series that update together on each bar,
    ensuring alignment across all series including custom indicators.

    Example:
        def on_bar_data(self, bar_data, bars):
            # Access OHLCV with reverse indexing
            current_close = bars.close[0]      # Most recent
            prev_close = bars.close[1]         # Previous bar

            # Built-in helpers
            sma20 = bars.sma(20)
            highest = bars.highest_high(14)
    """

    def __init__(self, symbol: str = "", max_lookback: int = 256):
        """
        Create a new BarsContext.

        Args:
            symbol: Trading symbol (e.g., "BTCUSDT")
            max_lookback: Maximum bars to keep (FIFO eviction)
        """
        self._symbol = symbol
        self._max_lookback = max_lookback
        self._current_bar = 0

        # OHLCV series
        self.open = DecimalSeries("Open", max_lookback)
        self.high = DecimalSeries("High", max_lookback)
        self.low = DecimalSeries("Low", max_lookback)
        self.close = DecimalSeries("Close", max_lookback)
        self.volume = DecimalSeries("Volume", max_lookback)
        self.time: Series[str] = Series("Time", max_lookback)
        self.trade_count: Series[int] = Series("TradeCount", max_lookback)

        # Custom series storage
        self._custom_series: Dict[str, Series] = {}

    def on_bar_update(self, bar_data: Dict[str, Any]) -> None:
        """
        Update all OHLCV series with new bar data.

        Called by the bridge before invoking strategy.on_bar_data().
        This ensures all series are synchronized at the same bar index.

        Args:
            bar_data: Bar data dictionary from Rust
        """
        self.open.push(Decimal(bar_data["open"]))
        self.high.push(Decimal(bar_data["high"]))
        self.low.push(Decimal(bar_data["low"]))
        self.close.push(Decimal(bar_data["close"]))
        self.volume.push(Decimal(bar_data["volume"]))
        self.time.push(bar_data["timestamp"])
        self.trade_count.push(bar_data["trade_count"])

        self._current_bar += 1

        # Update symbol on first bar
        if self._current_bar == 1:
            self._symbol = bar_data.get("symbol", self._symbol)

    # ========================================================================
    # Properties
    # ========================================================================

    def current_bar(self) -> int:
        """Get current bar index (0-based, increments with each bar)."""
        return self._current_bar

    def symbol(self) -> str:
        """Get the symbol being processed."""
        return self._symbol

    def count(self) -> int:
        """Get number of bars available."""
        return self.close.count()

    def has_bars(self, lookback: int) -> bool:
        """Check if we have enough data for given lookback."""
        return self.close.count() >= lookback

    # ========================================================================
    # Warmup / Ready State (QuantConnect Lean-style)
    # ========================================================================

    @property
    def is_ready(self) -> bool:
        """
        Check if BarsContext has at least 1 bar (basic readiness).

        Returns True when we have at least one bar of data.
        For more specific readiness checks, use `is_ready_for(lookback)`.
        """
        return self.count() > 0

    def is_ready_for(self, lookback: int) -> bool:
        """
        Check if we have enough bars for a specific lookback period.

        Use this to check if a specific indicator calculation will succeed.
        This is the primary method strategies should use to check warmup status.

        Args:
            lookback: Number of bars needed

        Returns:
            True if count() >= lookback

        Example:
            def is_ready(self, bars):
                # Ready when we have enough data for long SMA
                return bars.is_ready_for(self.long_period)
        """
        return self.count() >= lookback

    def sma_with_ready(self, period: int) -> tuple:
        """
        Calculate SMA and return (value, is_ready) tuple.

        Useful when you want to check both the value and whether it's valid
        in a single call.

        Args:
            period: SMA period

        Returns:
            Tuple of (SMA value or None, is_ready bool)
        """
        value = self.close.sma(period)
        ready = self.count() >= period
        return (value, ready)

    def highest_high_with_ready(self, period: int) -> tuple:
        """Get highest high with ready status."""
        value = self.high.highest(period)
        ready = self.count() >= period
        return (value, ready)

    def lowest_low_with_ready(self, period: int) -> tuple:
        """Get lowest low with ready status."""
        value = self.low.lowest(period)
        ready = self.count() >= period
        return (value, ready)

    def reset(self) -> None:
        """Reset all series (for new backtest run)."""
        self.open.reset()
        self.high.reset()
        self.low.reset()
        self.close.reset()
        self.volume.reset()
        self.time.reset()
        self.trade_count.reset()
        self._current_bar = 0
        self._custom_series.clear()

    # ========================================================================
    # Convenience indicator methods
    # ========================================================================

    def sma(self, period: int) -> Optional[Decimal]:
        """Calculate Simple Moving Average of close prices."""
        return self.close.sma(period)

    def sma_of(self, series: DecimalSeries, period: int) -> Optional[Decimal]:
        """Calculate SMA of a specific series."""
        return series.sma(period)

    def highest_high(self, period: int) -> Optional[Decimal]:
        """Get highest high over last N bars."""
        return self.high.highest(period)

    def lowest_low(self, period: int) -> Optional[Decimal]:
        """Get lowest low over last N bars."""
        return self.low.lowest(period)

    def highest_close(self, period: int) -> Optional[Decimal]:
        """Get highest close over last N bars."""
        return self.close.highest(period)

    def lowest_close(self, period: int) -> Optional[Decimal]:
        """Get lowest close over last N bars."""
        return self.close.lowest(period)

    def range(self) -> Optional[Decimal]:
        """Calculate price range (high - low) of current bar."""
        if self.high.is_empty() or self.low.is_empty():
            return None
        return self.high[0] - self.low[0]

    def average_range(self, period: int) -> Optional[Decimal]:
        """Calculate average true range (simplified: uses high-low)."""
        if self.count() < period or period == 0:
            return None

        total = Decimal(0)
        for i in range(period):
            h = self.high.get(i)
            l = self.low.get(i)
            if h is not None and l is not None:
                total += h - l

        return total / Decimal(period)

    def is_up_bar(self) -> bool:
        """Check if current bar is an up bar (close > open)."""
        if self.close.is_empty() or self.open.is_empty():
            return False
        return self.close[0] > self.open[0]

    def is_down_bar(self) -> bool:
        """Check if current bar is a down bar (close < open)."""
        if self.close.is_empty() or self.open.is_empty():
            return False
        return self.close[0] < self.open[0]

    def change(self) -> Optional[Decimal]:
        """Get price change from previous close."""
        if self.close.count() < 2:
            return None
        return self.close[0] - self.close[1]

    def percent_change(self) -> Optional[Decimal]:
        """Get percentage change from previous close."""
        if self.close.count() < 2:
            return None
        prev = self.close[1]
        if prev == Decimal(0):
            return None
        return (self.close[0] - prev) / prev * Decimal(100)

    # ========================================================================
    # Custom series management
    # ========================================================================

    def register_series(self, name: str, max_lookback: Optional[int] = None) -> DecimalSeries:
        """
        Register a new custom Decimal series.

        Args:
            name: Series name
            max_lookback: Optional custom lookback (uses context default if None)

        Returns:
            The created series for direct access
        """
        lookback = max_lookback if max_lookback is not None else self._max_lookback
        series = DecimalSeries(name, lookback)
        self._custom_series[name] = series
        return series

    def get_series(self, name: str) -> Optional[Series]:
        """Get a reference to a custom series."""
        return self._custom_series.get(name)

    def push_to_series(self, name: str, value: Any) -> bool:
        """
        Push a value to a custom series.

        Args:
            name: Series name
            value: Value to push

        Returns:
            True if successful, False if series not found
        """
        series = self._custom_series.get(name)
        if series is not None:
            series.push(value)
            return True
        return False

    def has_series(self, name: str) -> bool:
        """Check if a custom series exists."""
        return name in self._custom_series
