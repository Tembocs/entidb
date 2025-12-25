#!/usr/bin/env python3
"""
EntiDB Build and Test Script

This script automates the complete build and test workflow for EntiDB:
1. Builds Rust crates (including entidb_ffi)
2. Sets up a Python virtual environment for isolated testing
3. Builds Python bindings using maturin
4. Runs Python binding tests
5. Runs Dart binding tests (using the built native library)
6. Optionally runs example applications

Requirements:
- Rust toolchain (cargo)
- Python 3.8+
- uv (recommended) or pip for Python package management
- maturin (installed automatically if missing)
- Dart SDK

Usage:
    python build_and_test.py [options]

Options:
    --release           Build in release mode (default: debug)
    --skip-rust         Skip Rust build (use existing artifacts)
    --skip-rust-tests   Skip Rust tests
    --skip-python       Skip Python bindings build and test
    --skip-dart         Skip Dart bindings test
    --run-examples      Run example applications after tests
    --examples-only     Only run examples (skip tests, requires prior build)
    --python-version    Python version for bindings (default: 3.13)
    --clean-venv        Remove and recreate the Python virtual environment
    --verbose, -v       Show verbose output
    --help              Show this help message

Virtual Environment:
    The script creates a virtual environment at .venv-test in the project root.
    This isolates dependencies and ensures reproducible builds. Use --clean-venv
    to recreate it if you encounter issues.

Examples:
    # Full build and test
    python build_and_test.py

    # Build, test, and run examples
    python build_and_test.py --run-examples

    # Just run examples (after a previous build)
    python build_and_test.py --examples-only

    # Quick iteration on Dart only
    python build_and_test.py --skip-rust --skip-python
"""

import argparse
import os
import platform
import shutil
import subprocess
import sys
import venv
from pathlib import Path


# Enable ANSI colors on Windows 10+
def _enable_windows_ansi() -> bool:
    """Enable ANSI escape codes on Windows."""
    if platform.system() != "Windows":
        return True
    try:
        import ctypes
        kernel32 = ctypes.windll.kernel32
        # Enable ENABLE_VIRTUAL_TERMINAL_PROCESSING
        kernel32.SetConsoleMode(kernel32.GetStdHandle(-11), 7)
        return True
    except Exception:
        return False


# Detect if colors should be used
def _should_use_colors() -> bool:
    """Determine if ANSI colors should be used."""
    # Disable colors if NO_COLOR env var is set (standard)
    if os.environ.get("NO_COLOR"):
        return False
    # Disable colors if not a TTY (e.g., CI piped output)
    if not sys.stdout.isatty():
        return False
    # Try to enable Windows ANSI support
    return _enable_windows_ansi()


_USE_COLORS = _should_use_colors()


# ANSI colors for output
class Colors:
    HEADER = '\033[95m' if _USE_COLORS else ''
    BLUE = '\033[94m' if _USE_COLORS else ''
    CYAN = '\033[96m' if _USE_COLORS else ''
    GREEN = '\033[92m' if _USE_COLORS else ''
    WARNING = '\033[93m' if _USE_COLORS else ''
    FAIL = '\033[91m' if _USE_COLORS else ''
    END = '\033[0m' if _USE_COLORS else ''
    BOLD = '\033[1m' if _USE_COLORS else ''


def print_step(msg: str) -> None:
    """Print a step header."""
    print(f"\n{Colors.BOLD}{Colors.BLUE}==> {msg}{Colors.END}")


def print_success(msg: str) -> None:
    """Print a success message."""
    print(f"{Colors.GREEN}✓ {msg}{Colors.END}")


def print_error(msg: str) -> None:
    """Print an error message."""
    print(f"{Colors.FAIL}✗ {msg}{Colors.END}")


def print_warning(msg: str) -> None:
    """Print a warning message."""
    print(f"{Colors.WARNING}⚠ {msg}{Colors.END}")


def run_command(
    cmd: list[str],
    cwd: Path | None = None,
    env: dict | None = None,
    check: bool = True,
    verbose: bool = False,
    timeout: int | None = None,
) -> subprocess.CompletedProcess:
    """Run a command and handle errors."""
    if verbose:
        print(f"  Running: {' '.join(cmd)}")
    
    merged_env = os.environ.copy()
    if env:
        merged_env.update(env)
    
    try:
        result = subprocess.run(
            cmd,
            cwd=cwd,
            env=merged_env,
            check=check,
            capture_output=not verbose,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
        return result
    except subprocess.TimeoutExpired:
        print_error(f"Command timed out: {' '.join(cmd)}")
        raise
    except subprocess.CalledProcessError as e:
        print_error(f"Command failed: {' '.join(cmd)}")
        if e.stdout:
            print(f"stdout:\n{e.stdout}")
        if e.stderr:
            print(f"stderr:\n{e.stderr}")
        raise


def find_executable(name: str, alternatives: list[str] | None = None) -> str | None:
    """Find an executable in PATH."""
    candidates = [name] + (alternatives or [])
    for candidate in candidates:
        path = shutil.which(candidate)
        if path:
            return path
    return None


def check_dependencies(skip_rust: bool, skip_python: bool, skip_dart: bool, verbose: bool) -> bool:
    """
    Check that required build dependencies are available.
    
    Returns True if all required dependencies are found.
    """
    print_step("Checking dependencies")
    all_ok = True
    
    # Always need cargo for Rust
    if not skip_rust:
        cargo = find_executable("cargo")
        if cargo:
            if verbose:
                result = subprocess.run(
                    [cargo, "--version"],
                    capture_output=True,
                    text=True
                )
                print_success(f"Found cargo: {result.stdout.strip()}")
            else:
                print_success("Found cargo")
        else:
            print_error("cargo not found - install Rust from https://rustup.rs/")
            all_ok = False
    
    # Python checks
    if not skip_python:
        python = find_executable("python3", ["python"])
        if python:
            result = subprocess.run(
                [python, "--version"],
                capture_output=True,
                text=True
            )
            print_success(f"Found Python: {result.stdout.strip()}")
        else:
            print_error("python3 not found")
            all_ok = False
        
        # Check for maturin (will be installed in venv if missing, but good to note)
        maturin = find_executable("maturin")
        if maturin:
            print_success("Found maturin (global)")
        else:
            print_warning("maturin not found globally (will install in venv)")
    
    # Dart checks
    if not skip_dart:
        dart = find_executable("dart")
        if dart:
            result = subprocess.run(
                [dart, "--version"],
                capture_output=True,
                text=True
            )
            # Dart prints version to stderr
            version = result.stderr.strip() or result.stdout.strip()
            print_success(f"Found Dart: {version}")
        else:
            print_error("dart not found - install from https://dart.dev/get-dart")
            all_ok = False
    
    return all_ok


def get_venv_python(venv_dir: Path) -> Path:
    """Get the Python executable path inside a virtual environment."""
    if platform.system() == "Windows":
        return venv_dir / "Scripts" / "python.exe"
    else:
        return venv_dir / "bin" / "python"


def get_venv_pip(venv_dir: Path) -> Path:
    """Get the pip executable path inside a virtual environment."""
    if platform.system() == "Windows":
        return venv_dir / "Scripts" / "pip.exe"
    else:
        return venv_dir / "bin" / "pip"


def validate_venv_has_entidb(venv_dir: Path) -> bool:
    """Check if the virtual environment has entidb installed."""
    venv_python = get_venv_python(venv_dir)
    if not venv_python.exists():
        return False
    try:
        result = subprocess.run(
            [str(venv_python), "-c", "import entidb"],
            capture_output=True,
            text=True,
        )
        return result.returncode == 0
    except Exception:
        return False


def setup_python_venv(root: Path, python_version: str, verbose: bool) -> Path:
    """
    Set up a Python virtual environment for testing.
    
    Returns the path to the virtual environment directory.
    """
    venv_dir = root / ".venv-test"
    
    # Check if venv already exists and is valid
    venv_python = get_venv_python(venv_dir)
    if venv_python.exists():
        if verbose:
            print(f"  Using existing virtual environment: {venv_dir}")
        return venv_dir
    
    print_step("Setting up Python virtual environment")
    
    # Try using uv first (faster)
    uv = find_executable("uv")
    if uv:
        # Create venv with specific Python version using uv
        cmd = [uv, "venv", str(venv_dir), "--python", python_version]
        try:
            run_command(cmd, cwd=root, verbose=verbose)
            print_success(f"Created virtual environment with uv: {venv_dir}")
            return venv_dir
        except subprocess.CalledProcessError:
            print_warning(f"Failed to create venv with Python {python_version}, trying system Python")
            # Fall through to try with system Python
    
    # Fall back to standard venv module
    python = find_executable("python", ["python3"])
    if not python:
        print_error("Python not found. Please install Python 3.8+.")
        sys.exit(1)
    
    # Create venv using the venv module
    try:
        venv.create(venv_dir, with_pip=True, clear=True)
        print_success(f"Created virtual environment: {venv_dir}")
    except Exception as e:
        print_error(f"Failed to create virtual environment: {e}")
        sys.exit(1)
    
    return venv_dir


def install_venv_packages(venv_dir: Path, packages: list[str], verbose: bool) -> None:
    """Install packages into the virtual environment."""
    uv = find_executable("uv")
    
    if uv:
        # Use uv pip for faster installation
        cmd = [uv, "pip", "install", "--python", str(get_venv_python(venv_dir))] + packages
        run_command(cmd, verbose=verbose)
    else:
        # Use pip from the venv
        pip = get_venv_pip(venv_dir)
        if not pip.exists():
            print_error(f"pip not found in virtual environment: {pip}")
            sys.exit(1)
        cmd = [str(pip), "install"] + packages
        run_command(cmd, verbose=verbose)


def get_native_lib_path(root: Path, release: bool) -> Path:
    """Get the path to the built native library."""
    build_type = "release" if release else "debug"
    target_dir = root / "target" / build_type
    
    system = platform.system()
    if system == "Windows":
        return target_dir / "entidb_ffi.dll"
    elif system == "Darwin":
        return target_dir / "libentidb_ffi.dylib"
    else:  # Linux and others
        return target_dir / "libentidb_ffi.so"


def build_rust(root: Path, release: bool, verbose: bool) -> Path:
    """Build Rust crates including entidb_ffi."""
    print_step("Building Rust crates")
    
    cargo = find_executable("cargo")
    if not cargo:
        print_error("cargo not found. Please install Rust toolchain.")
        sys.exit(1)
    
    cmd = [cargo, "build", "-p", "entidb_ffi"]
    if release:
        cmd.append("--release")
    
    run_command(cmd, cwd=root, verbose=verbose)
    
    lib_path = get_native_lib_path(root, release)
    if lib_path.exists():
        print_success(f"Built native library: {lib_path}")
    else:
        print_error(f"Expected library not found: {lib_path}")
        sys.exit(1)
    
    return lib_path


def build_python_bindings(root: Path, venv_dir: Path, release: bool, verbose: bool) -> None:
    """Build Python bindings using maturin into the virtual environment."""
    print_step("Building Python bindings")
    
    python_dir = root / "bindings" / "python" / "entidb_py"
    venv_python = get_venv_python(venv_dir)
    
    # Ensure maturin is installed in the venv
    install_venv_packages(venv_dir, ["maturin"], verbose)
    
    # Run maturin develop using the venv Python
    # Use VIRTUAL_ENV env var to ensure maturin installs to our venv
    # This is needed because maturin may create its own .venv otherwise
    env = {"VIRTUAL_ENV": str(venv_dir)}
    
    cmd = [str(venv_python), "-m", "maturin", "develop"]
    if release:
        cmd.append("--release")
    run_command(cmd, cwd=python_dir, env=env, verbose=verbose)
    
    print_success("Python bindings built successfully")


def test_python_bindings(root: Path, venv_dir: Path, verbose: bool) -> bool:
    """Run Python binding tests using the virtual environment."""
    print_step("Running Python binding tests")
    
    python_dir = root / "bindings" / "python" / "entidb_py"
    venv_python = get_venv_python(venv_dir)
    
    # Ensure pytest is installed
    install_venv_packages(venv_dir, ["pytest"], verbose)
    
    # Run pytest using the venv Python
    cmd = [str(venv_python), "-m", "pytest", "-v"]
    result = run_command(cmd, cwd=python_dir, verbose=verbose, check=False)
    
    if result.returncode == 0:
        print_success("Python tests passed")
        return True
    else:
        print_error("Python tests failed")
        if not verbose and result.stdout:
            print(result.stdout)
        if not verbose and result.stderr:
            print(result.stderr)
        return False


def test_dart_bindings(root: Path, lib_path: Path, verbose: bool) -> bool:
    """Run Dart binding tests."""
    print_step("Running Dart binding tests")
    
    dart = find_executable("dart")
    if not dart:
        print_warning("Dart SDK not found. Skipping Dart tests.")
        return True
    
    dart_dir = root / "bindings" / "dart" / "entidb_dart"
    
    # Get dependencies first
    run_command([dart, "pub", "get"], cwd=dart_dir, verbose=verbose)
    
    # Set environment variable for library path
    env = {"ENTIDB_LIB_PATH": str(lib_path)}
    
    cmd = [dart, "test", "-r", "expanded"]
    result = run_command(cmd, cwd=dart_dir, env=env, verbose=verbose, check=False)
    
    if result.returncode == 0:
        print_success("Dart tests passed")
        return True
    else:
        print_error("Dart tests failed")
        if not verbose and result.stdout:
            print(result.stdout)
        if not verbose and result.stderr:
            print(result.stderr)
        return False


def run_rust_tests(root: Path, verbose: bool) -> bool:
    """Run Rust tests."""
    print_step("Running Rust tests")
    
    cargo = find_executable("cargo")
    if not cargo:
        print_error("cargo not found")
        return False
    
    # Exclude Python binding (entidb_py) from workspace tests since it requires
    # a specific Python version that pyo3 supports
    result = run_command(
        [cargo, "test", "--workspace", "--exclude", "entidb_py"],
        cwd=root,
        verbose=verbose,
        check=False,
    )
    
    if result.returncode == 0:
        print_success("Rust tests passed")
        return True
    else:
        print_error("Rust tests failed")
        return False


def run_rust_examples(root: Path, verbose: bool) -> bool:
    """Run Rust examples."""
    print_step("Running Rust examples")
    
    cargo = find_executable("cargo")
    if not cargo:
        print_error("cargo not found")
        return False
    
    examples = ["rust_todo", "rust_notes"]
    all_passed = True
    
    for example in examples:
        example_dir = root / "examples" / example
        if not example_dir.exists():
            print_warning(f"Example not found: {example}")
            continue
        
        print(f"  Running {example}...")
        result = run_command(
            [cargo, "run"],
            cwd=example_dir,
            verbose=verbose,
            check=False,
        )
        
        if result.returncode == 0:
            print_success(f"{example} completed successfully")
        else:
            print_error(f"{example} failed")
            all_passed = False
    
    return all_passed


def run_python_examples(root: Path, venv_dir: Path, verbose: bool) -> bool:
    """Run Python examples using the virtual environment."""
    print_step("Running Python examples")
    
    venv_python = get_venv_python(venv_dir)
    if not venv_python.exists():
        print_error("Python virtual environment not found. Run without --skip-python first.")
        return False
    
    examples = ["python_todo"]
    all_passed = True
    
    for example in examples:
        example_dir = root / "examples" / example
        main_file = example_dir / "main.py"
        
        if not main_file.exists():
            print_warning(f"Example not found: {example}")
            continue
        
        print(f"  Running {example}...")
        result = run_command(
            [str(venv_python), str(main_file)],
            cwd=example_dir,
            verbose=verbose,
            check=False,
        )
        
        if result.returncode == 0:
            print_success(f"{example} completed successfully")
        else:
            print_error(f"{example} failed")
            if not verbose and result.stdout:
                print(result.stdout)
            if not verbose and result.stderr:
                print(result.stderr)
            all_passed = False
    
    return all_passed


def run_dart_examples(root: Path, lib_path: Path, verbose: bool) -> bool:
    """Run Dart examples with the native library."""
    print_step("Running Dart examples")
    
    dart = find_executable("dart")
    if not dart:
        print_warning("Dart SDK not found. Skipping Dart examples.")
        return True
    
    examples = ["dart_todo"]
    all_passed = True
    
    for example in examples:
        example_dir = root / "examples" / example
        main_file = example_dir / "main.dart"
        
        if not main_file.exists():
            print_warning(f"Example not found: {example}")
            continue
        
        print(f"  Running {example}...")
        
        # Get dependencies first
        run_command([dart, "pub", "get"], cwd=example_dir, verbose=verbose)
        
        # Set environment variable for library path
        env = {"ENTIDB_LIB_PATH": str(lib_path)}
        
        result = run_command(
            [dart, "run", str(main_file)],
            cwd=example_dir,
            env=env,
            verbose=verbose,
            check=False,
        )
        
        if result.returncode == 0:
            print_success(f"{example} completed successfully")
        else:
            print_error(f"{example} failed")
            if not verbose and result.stdout:
                print(result.stdout)
            if not verbose and result.stderr:
                print(result.stderr)
            all_passed = False
    
    return all_passed


def main() -> int:
    """Main entry point."""
    parser = argparse.ArgumentParser(
        description="Build and test EntiDB",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    parser.add_argument(
        "--release",
        action="store_true",
        help="Build in release mode (default: debug)",
    )
    parser.add_argument(
        "--skip-rust",
        action="store_true",
        help="Skip Rust build (use existing artifacts)",
    )
    parser.add_argument(
        "--skip-rust-tests",
        action="store_true",
        help="Skip Rust tests",
    )
    parser.add_argument(
        "--skip-python",
        action="store_true",
        help="Skip Python bindings build and test",
    )
    parser.add_argument(
        "--skip-dart",
        action="store_true",
        help="Skip Dart bindings test",
    )
    parser.add_argument(
        "--run-examples",
        action="store_true",
        help="Run example applications after tests",
    )
    parser.add_argument(
        "--examples-only",
        action="store_true",
        help="Only run examples (skip tests, requires prior build)",
    )
    parser.add_argument(
        "--python-version",
        default="3.13",
        help="Python version to use for bindings (default: 3.13, pyo3 may not support newer versions)",
    )
    parser.add_argument(
        "--clean-venv",
        action="store_true",
        help="Remove and recreate the Python virtual environment",
    )
    parser.add_argument(
        "--verbose", "-v",
        action="store_true",
        help="Show verbose output",
    )
    
    args = parser.parse_args()
    
    # Find project root (where this script is located)
    root = Path(__file__).parent.resolve()
    
    # Handle clean-venv early (applies to all modes)
    venv_path = root / ".venv-test"
    if args.clean_venv and venv_path.exists():
        print_step("Cleaning existing virtual environment")
        shutil.rmtree(venv_path)
        print_success("Removed old virtual environment")
    
    # Determine mode
    examples_only = args.examples_only
    run_examples = args.run_examples or examples_only
    run_tests = not examples_only
    
    # Validate mode combinations
    if examples_only and args.clean_venv and not args.skip_python:
        print_error("Cannot use --clean-venv with --examples-only (no way to rebuild).")
        print_error("Either remove --clean-venv or run a full build first.")
        return 1
    
    print(f"{Colors.BOLD}{Colors.HEADER}")
    print("=" * 60)
    if examples_only:
        print("  EntiDB Examples")
    elif run_examples:
        print("  EntiDB Build, Test, and Examples")
    else:
        print("  EntiDB Build and Test")
    print("=" * 60)
    print(f"{Colors.END}")
    print(f"Project root: {root}")
    print(f"Build type: {'release' if args.release else 'debug'}")
    print(f"Python version: {args.python_version}")
    
    # Dependency check (skip in examples-only mode since we just run existing builds)
    if not examples_only:
        if not check_dependencies(args.skip_rust, args.skip_python, args.skip_dart, args.verbose):
            print_error("Missing required dependencies. Install them and try again.")
            return 1
    
    all_passed = True
    lib_path: Path | None = None
    venv_dir: Path | None = None
    
    # Build Rust (unless examples-only mode with existing build)
    if examples_only:
        # In examples-only mode, just check for existing build
        lib_path = get_native_lib_path(root, args.release)
        if lib_path.exists():
            print_step("Using existing Rust build")
            print_success(f"Found native library: {lib_path}")
        else:
            print_error(f"Native library not found: {lib_path}")
            print_error("Run without --examples-only to build first.")
            return 1
        
        # Check for existing venv
        venv_path = root / ".venv-test"
        if venv_path.exists():
            venv_dir = venv_path
            print_success(f"Found virtual environment: {venv_dir}")
        else:
            print_warning("No virtual environment found. Python examples will be skipped.")
    elif not args.skip_rust:
        lib_path = build_rust(root, args.release, args.verbose)
    else:
        lib_path = get_native_lib_path(root, args.release)
        if lib_path.exists():
            print_step("Using existing Rust build")
            print_success(f"Found native library: {lib_path}")
        else:
            print_error(f"Native library not found: {lib_path}")
            print_error("Run without --skip-rust to build it first.")
            return 1
    
    # Run Rust tests
    if run_tests and not args.skip_rust_tests:
        if not run_rust_tests(root, args.verbose):
            all_passed = False
    
    # Build and test Python bindings
    if run_tests and not args.skip_python:
        try:
            # Set up virtual environment (clean_venv already handled above)
            venv_dir = setup_python_venv(root, args.python_version, args.verbose)
            
            # Build and test
            build_python_bindings(root, venv_dir, args.release, args.verbose)
            if not test_python_bindings(root, venv_dir, args.verbose):
                all_passed = False
        except subprocess.CalledProcessError:
            all_passed = False
    
    # Test Dart bindings
    if run_tests and not args.skip_dart:
        if not test_dart_bindings(root, lib_path, args.verbose):
            all_passed = False
    
    # Run examples
    if run_examples:
        print(f"\n{Colors.BOLD}{Colors.CYAN}--- Running Examples ---{Colors.END}\n")
        
        # Rust examples (always available, they use workspace deps)
        if not run_rust_examples(root, args.verbose):
            all_passed = False
        
        # Python examples (need venv with entidb)
        if not args.skip_python and venv_dir is not None:
            if validate_venv_has_entidb(venv_dir):
                if not run_python_examples(root, venv_dir, args.verbose):
                    all_passed = False
            else:
                print_warning("Skipping Python examples (entidb not installed in venv)")
                print_warning("Run without --examples-only to build Python bindings first.")
        elif not args.skip_python and venv_dir is None:
            print_warning("Skipping Python examples (no virtual environment)")
        
        # Dart examples (need native library)
        if not args.skip_dart:
            if not run_dart_examples(root, lib_path, args.verbose):
                all_passed = False
    
    # Summary
    print(f"\n{Colors.BOLD}")
    print("=" * 60)
    if all_passed:
        if run_examples:
            print(f"{Colors.GREEN}  All builds, tests, and examples passed!{Colors.END}")
        else:
            print(f"{Colors.GREEN}  All builds and tests passed!{Colors.END}")
    else:
        print(f"{Colors.FAIL}  Some builds, tests, or examples failed.{Colors.END}")
    print("=" * 60)
    print(Colors.END)
    
    return 0 if all_passed else 1


if __name__ == "__main__":
    sys.exit(main())
