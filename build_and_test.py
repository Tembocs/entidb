#!/usr/bin/env python3
"""
EntiDB Build and Test Script

This script automates the complete build and test workflow for EntiDB:
1. Builds Rust crates (including entidb_ffi)
2. Builds Python bindings using maturin
3. Runs Python binding tests
4. Runs Dart binding tests (using the built native library)

Requirements:
- Rust toolchain (cargo)
- Python 3.8+
- uv (Python package manager) or pip
- maturin (installed automatically if missing)
- Dart SDK

Usage:
    python build_and_test.py [options]

Options:
    --release       Build in release mode (default: debug)
    --skip-rust     Skip Rust build (use existing artifacts)
    --skip-python   Skip Python bindings build and test
    --skip-dart     Skip Dart bindings test
    --verbose       Show verbose output
    --help          Show this help message
"""

import argparse
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path


# ANSI colors for output
class Colors:
    HEADER = '\033[95m'
    BLUE = '\033[94m'
    CYAN = '\033[96m'
    GREEN = '\033[92m'
    WARNING = '\033[93m'
    FAIL = '\033[91m'
    END = '\033[0m'
    BOLD = '\033[1m'


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
        )
        return result
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


def build_python_bindings(root: Path, release: bool, verbose: bool, python_version: str = "3.13") -> None:
    """Build Python bindings using maturin."""
    print_step("Building Python bindings")
    
    python_dir = root / "bindings" / "python" / "entidb_py"
    
    # Check for uv or pip
    uv = find_executable("uv")
    
    if uv:
        # Use uv to run maturin with specific Python version
        # Note: pyo3 may not support the latest Python, so we default to 3.13
        cmd = [uv, "run", "--python", python_version, "--isolated", "--with", "maturin", "maturin", "develop"]
        if release:
            cmd.append("--release")
        run_command(cmd, cwd=python_dir, verbose=verbose)
    else:
        # Fall back to pip/maturin directly
        maturin = find_executable("maturin")
        if not maturin:
            print_warning("maturin not found. Installing via pip...")
            pip = find_executable("pip", ["pip3"])
            if not pip:
                print_error("pip not found. Please install Python properly.")
                sys.exit(1)
            run_command([pip, "install", "maturin"], verbose=verbose)
            maturin = find_executable("maturin")
        
        if not maturin:
            print_error("Failed to install maturin")
            sys.exit(1)
        
        cmd = [maturin, "develop"]
        if release:
            cmd.append("--release")
        run_command(cmd, cwd=python_dir, verbose=verbose)
    
    print_success("Python bindings built successfully")


def test_python_bindings(root: Path, verbose: bool, python_version: str = "3.13") -> bool:
    """Run Python binding tests."""
    print_step("Running Python binding tests")
    
    python_dir = root / "bindings" / "python" / "entidb_py"
    
    uv = find_executable("uv")
    
    if uv:
        cmd = [uv, "run", "--python", python_version, "--isolated", "--with", "pytest", "pytest", "-v"]
        result = run_command(cmd, cwd=python_dir, verbose=verbose, check=False)
    else:
        pytest = find_executable("pytest")
        if not pytest:
            print_warning("pytest not found. Installing...")
            pip = find_executable("pip", ["pip3"])
            run_command([pip, "install", "pytest"], verbose=verbose)
            pytest = find_executable("pytest")
        
        if not pytest:
            print_error("Failed to install pytest")
            return False
        
        result = run_command([pytest, "-v"], cwd=python_dir, verbose=verbose, check=False)
    
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
        "--python-version",
        default="3.13",
        help="Python version to use for bindings (default: 3.13, pyo3 may not support newer versions)",
    )
    parser.add_argument(
        "--verbose", "-v",
        action="store_true",
        help="Show verbose output",
    )
    
    args = parser.parse_args()
    
    # Find project root (where this script is located)
    root = Path(__file__).parent.resolve()
    
    print(f"{Colors.BOLD}{Colors.HEADER}")
    print("=" * 60)
    print("  EntiDB Build and Test")
    print("=" * 60)
    print(f"{Colors.END}")
    print(f"Project root: {root}")
    print(f"Build type: {'release' if args.release else 'debug'}")
    
    all_passed = True
    lib_path: Path | None = None
    
    # Build Rust
    if not args.skip_rust:
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
    if not args.skip_rust_tests:
        if not run_rust_tests(root, args.verbose):
            all_passed = False
    
    # Build and test Python bindings
    if not args.skip_python:
        try:
            build_python_bindings(root, args.release, args.verbose, args.python_version)
            if not test_python_bindings(root, args.verbose, args.python_version):
                all_passed = False
        except subprocess.CalledProcessError:
            all_passed = False
    
    # Test Dart bindings
    if not args.skip_dart:
        if not test_dart_bindings(root, lib_path, args.verbose):
            all_passed = False
    
    # Summary
    print(f"\n{Colors.BOLD}")
    print("=" * 60)
    if all_passed:
        print(f"{Colors.GREEN}  All builds and tests passed!{Colors.END}")
    else:
        print(f"{Colors.FAIL}  Some builds or tests failed.{Colors.END}")
    print("=" * 60)
    print(Colors.END)
    
    return 0 if all_passed else 1


if __name__ == "__main__":
    sys.exit(main())
