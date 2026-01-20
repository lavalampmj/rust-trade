"""
Strategy library infrastructure.

This package contains internal infrastructure for Python strategies:
- base_strategy: Base classes and Series abstractions
- restricted_compiler: Security sandbox for strategy compilation
"""

from .base_strategy import (
    BaseStrategy,
    Signal,
    Series,
    DecimalSeries,
    FloatSeries,
)

__all__ = [
    "BaseStrategy",
    "Signal",
    "Series",
    "DecimalSeries",
    "FloatSeries",
]
