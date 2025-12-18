# entidb_codec

Canonical CBOR encoding/decoding for EntiDB.

## Overview

This crate provides a strict canonical CBOR implementation following [RFC 8949 Section 4.2.1](https://www.rfc-editor.org/rfc/rfc8949.html#section-4.2.1) deterministic encoding rules. It is designed specifically for EntiDB's requirements where byte-exact reproducibility is mandatory.

## Features

- **Deterministic encoding**: Identical logical values produce identical bytes
- **Canonical map ordering**: Keys sorted by encoded form (length-first, then bytewise)
- **Shortest integer encoding**: Integers use minimum bytes required
- **Strict validation**: Decoder rejects non-canonical input
- **No floats**: Floating-point values are forbidden per EntiDB spec

## Usage

```rust
use entidb_codec::{to_canonical_cbor, from_cbor, Value};

// Create a value
let value = Value::map(vec![
    (Value::Text("name".into()), Value::Text("Alice".into())),
    (Value::Integer(42), Value::Bool(true)),
]);

// Encode to canonical CBOR bytes
let bytes = to_canonical_cbor(&value)?;

// Decode back (validates canonicity)
let decoded = from_cbor(&bytes)?;
assert_eq!(value, decoded);
```

## Value Types

| Type | Description |
|------|-------------|
| `Value::Null` | CBOR null (0xf6) |
| `Value::Bool(bool)` | CBOR true/false |
| `Value::Integer(i64)` | Signed integer (types 0 and 1) |
| `Value::Bytes(Vec<u8>)` | Byte string (type 2) |
| `Value::Text(String)` | UTF-8 text string (type 3) |
| `Value::Array(Vec<Value>)` | Array (type 4) |
| `Value::Map(BTreeMap<Value, Value>)` | Map (type 5, sorted) |

## Canonical Rules

1. **Integers**: Use shortest encoding (0-23 in one byte, 24-255 in two bytes, etc.)
2. **Maps**: Keys sorted by encoded form - shorter encodings first, then bytewise comparison
3. **Strings**: UTF-8 text, definite-length only
4. **Forbidden**: Floats, NaN, indefinite-length items, non-shortest encoding

## Error Handling

The decoder returns `CodecError` for:
- `FloatForbidden` - Float values encountered
- `IndefiniteLengthForbidden` - Indefinite-length encoding used  
- `InvalidUtf8` - Non-UTF-8 text string
- `InvalidStructure` - Non-canonical encoding (unsorted keys, non-shortest integers)
- `UnexpectedEof` - Truncated input

## Relation to EntiDB

This crate is a foundational dependency for `entidb_core`. All entity payloads stored in EntiDB use this canonical encoding to ensure:

- Hash stability across languages
- Deterministic segment files
- Consistent sync protocol messages

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
