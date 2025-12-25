# entidb_ffi

Stable C ABI for EntiDB bindings.

## Overview

This crate provides a stable C ABI interface for EntiDB, enabling language bindings
(Dart, Python, and others) to interact with the EntiDB core engine safely.

## Features

- **Stable C ABI**: Consistent interface across different Rust compiler versions
- **Memory-safe FFI**: Explicit ownership and buffer management
- **Error codes**: ABI-safe error handling without panics across FFI boundary
- **Encryption support**: Optional AES-256-GCM encryption APIs

## Usage

This crate is intended for binding authors. See the [Dart](../../bindings/dart/entidb_dart)
and [Python](../../bindings/python/entidb_py) bindings for reference implementations.

## Building

The crate produces both `cdylib` (shared library) and `staticlib` outputs:

```bash
cargo build --release -p entidb_ffi
```

## Safety

This crate contains `unsafe` code as required for FFI. All unsafe operations are
carefully documented and reviewed.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
