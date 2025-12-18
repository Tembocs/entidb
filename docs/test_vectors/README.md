# EntiDB Cross-Language Test Vectors

This directory contains test vectors for validating identical behavior across
Rust, Dart, and Python bindings.

## Vector Files

| File | Description |
|------|-------------|
| [cbor.json](cbor.json) | Canonical CBOR encoding test vectors |
| [entity_id.json](entity_id.json) | Entity ID serialization vectors |
| [wal.json](wal.json) | WAL record format vectors |
| [segment.json](segment.json) | Segment record format vectors |

## Usage

### Rust

```rust
use entidb_testkit::vectors::{cbor_encoding_vectors, entity_id_vectors};

#[test]
fn test_cbor_parity() {
    for vector in cbor_encoding_vectors() {
        // Test decode + re-encode produces expected output
    }
}
```

### Dart

```dart
import 'dart:convert';
import 'dart:io';

void main() {
  final vectors = jsonDecode(File('cbor.json').readAsStringSync());
  for (final vector in vectors) {
    // Test CBOR encoding matches expected output
  }
}
```

### Python

```python
import json

with open('cbor.json') as f:
    vectors = json.load(f)

for vector in vectors:
    # Test CBOR encoding matches expected output
    pass
```

## Vector Format

Each test vector has the following structure:

```json
{
  "id": "unique_test_id",
  "description": "Human-readable description",
  "input_hex": "hexadecimal input bytes",
  "expected_hex": "expected output bytes (hex)",
  "expected_error": null  // or error message if should fail
}
```

## Generating Vectors

Vectors are generated from Rust and exported to JSON:

```bash
cargo test -p entidb_testkit export_vectors -- --ignored
```

## Validation Requirements

All bindings MUST pass these vectors with identical behavior:

1. **CBOR Encoding**: Identical canonical CBOR output bytes
2. **Entity ID**: Same 16-byte serialization
3. **WAL Records**: Matching record structure and checksums
4. **Segment Records**: Matching layout and flags

See [bindings_contract.md](../bindings_contract.md) for full requirements.
