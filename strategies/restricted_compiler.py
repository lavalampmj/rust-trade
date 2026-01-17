"""
Restricted Python Compiler for Strategy Sandboxing

This module provides a secure compilation environment for Python trading strategies
using RestrictedPython. It blocks dangerous imports and operations while allowing
safe mathematical and algorithmic operations.

Security layers:
1. Import blocking - Prevents importing dangerous system/network modules
2. Safe globals - Restricts available built-in functions
3. Compilation restrictions - Prevents dynamic code execution
"""

from RestrictedPython import compile_restricted, safe_globals
from RestrictedPython.Guards import guarded_iter_unpack_sequence

# BLOCKED modules (security threats)
# These modules provide system access, network access, or code execution capabilities
BLOCKED_MODULES = {
    # System access
    'os', 'sys', 'subprocess', 'shutil', 'pathlib',
    'pty', 'pwd', 'grp', 'resource', 'signal',

    # Network access
    'urllib', 'urllib.request', 'urllib2', 'urllib3',
    'http', 'httplib', 'httplib2', 'requests', 'socket', 'socketserver',
    'ftplib', 'smtplib', 'poplib', 'imaplib', 'telnetlib',
    'ssl', 'asyncio', 'aiohttp',

    # Code execution
    'eval', 'exec', 'compile', '__import__',
    'importlib', 'imp',

    # File I/O (beyond what's needed)
    'io', 'open', 'file', 'fileinput', 'tempfile',
    'zipfile', 'tarfile', 'gzip', 'bz2',

    # Serialization (can be used for RCE)
    'pickle', 'marshal', 'shelve', 'dbm',

    # FFI and low-level access
    'ctypes', 'cffi', '_ctypes',

    # Concurrency (could bypass limits)
    'multiprocessing', 'threading', 'concurrent',
    '_thread', 'queue',

    # Other dangerous modules
    'webbrowser', 'cgi', 'cgitb',
    'code', 'codeop', 'pdb', 'profile',
    'gc', 'inspect', 'traceback',
}

# ALLOWED modules (explicitly safe for strategies)
# These are common Python stdlib modules that are safe for mathematical/algorithmic operations
ALLOWED_MODULES = {
    # Math and numbers
    'math', 'cmath', 'decimal', 'fractions', 'numbers', 'random', 'statistics',

    # Data structures
    'collections', 'heapq', 'bisect', 'array', 'enum',

    # Functional programming
    'itertools', 'functools', 'operator',

    # String processing
    'string', 're', 'difflib', 'textwrap',

    # Date and time
    'datetime', 'time', 'calendar',

    # Typing
    'typing', 'types',

    # Data persistence (safe ones)
    'json', 'csv',

    # Other safe modules
    'copy', 'pprint', 'repr',
}


def safe_write(obj):
    """
    Safe write guard for attribute assignment.

    This is simpler than full_write_guard - it allows normal attribute assignment
    without requiring __guarded_setattr__, which is needed for strategy classes.

    Returns the object unchanged to allow write operations.
    """
    return obj


def restricted_import(name, globals=None, locals=None, fromlist=(), level=0):
    """
    Custom import guard that blocks dangerous imports.

    This function is called whenever Python code tries to import a module.
    It checks against the BLOCKED_MODULES list and raises ImportError for
    dangerous modules.

    Args:
        name: Module name being imported
        globals: Global namespace (not used)
        locals: Local namespace (not used)
        fromlist: List of names to import from module
        level: Relative import level

    Returns:
        Imported module if allowed

    Raises:
        ImportError: If module is in BLOCKED_MODULES list
    """
    # Check if module or any parent is blocked
    for blocked in BLOCKED_MODULES:
        if name == blocked or name.startswith(blocked + '.'):
            raise ImportError(
                f"Import of '{name}' is blocked for security reasons.\n"
                f"Blocked module categories: system access, network access, file I/O, "
                f"code execution, serialization, FFI, concurrency.\n"
                f"See strategies/restricted_compiler.py for full list of blocked modules."
            )

    # Allow the import using the built-in __import__
    # We use the original built-in __import__ from the builtins module
    import builtins
    return builtins.__import__(name, globals, locals, fromlist, level)


def compile_strategy(code: str, filename: str):
    """
    Compile Python strategy with restrictions.

    This function takes Python source code and compiles it in a restricted
    environment using RestrictedPython. The compiled code will:
    - Have restricted imports (via restricted_import)
    - Use safe globals (limited built-in functions)
    - Cannot use eval/exec or other dynamic code execution

    Args:
        code: Python source code as string
        filename: Name of the file (for error messages)

    Returns:
        tuple: (compiled_code, restricted_globals)
            - compiled_code: Compiled bytecode ready for execution
            - restricted_globals: Restricted global namespace dictionary

    Raises:
        ValueError: If compilation fails due to restricted syntax
    """
    # Compile with RestrictedPython
    # Note: compile_restricted returns a code object directly in RestrictedPython 7.0
    # It doesn't check imports at compile time - that's done at runtime via __import__
    try:
        byte_code = compile_restricted(
            code,
            filename=filename,
            mode='exec',
        )
    except SyntaxError as e:
        raise ValueError(
            f"Syntax error in strategy code:\n{e}\n\n"
            f"Please check your Python syntax."
        )

    # Create restricted globals starting with safe_globals as base
    from copy import deepcopy
    restricted_globals = deepcopy(safe_globals)

    # Add additional safe built-ins that aren't in safe_globals
    restricted_globals['__builtins__'].update({
        # Additional safe built-in functions
        'all': all,
        'any': any,
        'dict': dict,
        'enumerate': enumerate,
        'list': list,
        'max': max,
        'min': min,
        'reversed': reversed,
        'set': set,
        'sum': sum,
        'type': type,

        # Custom import guard (override default __import__)
        '__import__': restricted_import,
    })

    # Add RestrictedPython guards and special variables
    import operator
    restricted_globals.update({
        '_getiter_': guarded_iter_unpack_sequence,
        '_iter_unpack_sequence_': guarded_iter_unpack_sequence,
        '_getitem_': operator.getitem,  # Required for subscript operations like Dict[str, str]
        '_write_': safe_write,  # Required for attribute assignment
        '__metaclass__': type,  # Required by RestrictedPython for class definitions
        '__name__': '__main__',  # Default module name
    })

    return byte_code, restricted_globals


def get_blocked_modules_list():
    """
    Get the list of blocked modules for documentation/debugging.

    Returns:
        set: Set of blocked module names
    """
    return BLOCKED_MODULES.copy()


def get_allowed_modules_list():
    """
    Get the list of explicitly allowed modules for documentation/debugging.

    Returns:
        set: Set of allowed module names
    """
    return ALLOWED_MODULES.copy()
