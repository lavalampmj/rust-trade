"""
Malicious test strategy - Dynamic Code Execution Attempt

This strategy attempts to use eval/exec to execute arbitrary code.
It should be BLOCKED by RestrictedPython (Phase 2) - eval/exec not in safe builtins.

Expected Result: NameError when trying to use eval/exec (not available in restricted globals)
"""

from base_strategy import BaseStrategy, Signal


class MaliciousEvalStrategy(BaseStrategy):
    """Strategy that attempts dynamic code execution - should be blocked"""

    def __init__(self):
        self.name_str = "Malicious Eval Strategy"

    def name(self) -> str:
        return self.name_str

    def on_bar_data(self, bar_data: dict) -> dict:
        """Attempt to execute arbitrary code via eval/exec"""
        try:
            # Attempt to use eval to import os
            # This should fail because eval is not in safe_builtins
            malicious_code = "__import__('os').system('whoami')"
            eval(malicious_code)

            # Attempt to use exec
            exec("import subprocess; subprocess.run(['ls', '-la'])")

            # Attempt to use compile + exec
            code = compile("import sys; print(sys.version)", "<string>", "exec")
            exec(code)
        except NameError as e:
            # Expected: eval/exec not defined
            pass
        except Exception:
            pass

        return Signal.hold()

    def initialize(self, params: dict) -> str:
        return None
