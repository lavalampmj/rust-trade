"""
Strategy library infrastructure.

This package contains internal infrastructure for Python strategies:
- base_strategy: Base classes and Series abstractions
- bars_context: BarsContext for synchronized OHLCV series access
- restricted_compiler: Security sandbox for strategy compilation
- ComponentState: Unified component lifecycle states
"""

from .base_strategy import (
    BaseStrategy,
    Signal,
    Series,
    DecimalSeries,
    FloatSeries,
    ComponentState,
)
from .bars_context import BarsContext

__all__ = [
    "BaseStrategy",
    "Signal",
    "Series",
    "DecimalSeries",
    "FloatSeries",
    "BarsContext",
    "ComponentState",
]
