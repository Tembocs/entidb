# EntiDB Rust Notes Example

A notes application demonstrating advanced EntiDB features.

## Requirements

- Rust 1.75+

## Running

```bash
cargo run -p rust_notes
```

## Features Demonstrated

- Complex entities with arrays (tags)
- CBOR encoding/decoding for nested structures
- Tag-based filtering with `filter()` and closures
- Content search with string matching
- Entity updates within transactions
- Statistics aggregation with iterators
- **No SQL** - pure Rust data manipulation

## Key Concepts

### Complex Entity Encoding

Notes include an array of tags, demonstrating how to encode/decode
nested structures with CBOR:

```rust
let tags_array: Vec<Value> = self.tags.iter()
    .map(|t| Value::Text(t.clone()))
    .collect();
```

### Filtering by Tag

```rust
let work_notes: Vec<&Note> = all_notes
    .iter()
    .filter(|n| n.has_tag("work"))
    .collect();
```

### Content Search

```rust
let search_results: Vec<&Note> = all_notes
    .iter()
    .filter(|n| n.content.to_lowercase().contains("database"))
    .collect();
```
