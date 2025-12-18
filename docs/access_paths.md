# Index Selection and Access Path Policy (Normative)

This document defines how EntiDB selects access paths **without exposing any query language**.

This document is **normative**. All implementations **MUST** conform to this specification.

---

## 1. Fundamental Principles

### 1.1 No Query Language

EntiDB **MUST NOT** expose any query language, query builder, or DSL.

* **NO SQL**
* **NO SQL-like APIs**
* **NO query builders**
* **NO string-based predicates**

### 1.2 Engine-Controlled Selection

* The engine, not the user, selects access paths.
* Users **MUST NOT** reference indexes by name during queries.
* Index existence **MUST NOT** change query results (only performance).

### 1.3 Host-Language Filtering

Filtering is performed using host-language constructs:

* **Rust**: Iterator adapters (`filter`, `map`, `take`, `skip`)
* **Dart**: Collection methods (`where`, `map`, `take`)
* **Python**: Comprehensions and generator expressions

---

## 2. Access Path Types

### 2.1 Full Scan

Iterates the entire collection sequentially.

**Characteristics:**

* O(n) complexity where n = collection size
* No index required
* Always available as fallback

**API Requirement:**

* Full scans **MUST** be explicit in the API.
* Users **MUST** opt-in to scan behavior.
* Accidental full scans **MUST** be preventable.

```rust
// Explicit scan API (Rust example)
collection.scan()              // Returns iterator over all entities
collection.scan_with_limit(100) // Bounded scan
```

### 2.2 Primary Key Access

Direct lookup by entity ID.

**Characteristics:**

* O(1) average complexity (hash-based)
* Always available (entity ID is mandatory)
* Most efficient access path

```rust
// Primary key access (Rust example)
collection.get(entity_id)       // Returns Option<Entity>
collection.get_many(&[id1, id2]) // Batch lookup
```

### 2.3 Hash Index Access

Equality-based lookup on indexed fields.

**Characteristics:**

* O(1) average complexity
* Requires hash index on target field
* Exact match only (no ranges)

**Use Cases:**

* Lookup by unique identifier
* Equality filters on high-cardinality fields
* Foreign key relationships

```rust
// Hash index access (Rust example)
collection.find_by_email("user@example.com")  // Single result
collection.find_all_by_status("active")       // Multiple results
```

### 2.4 BTree Index Access

Ordered lookup supporting ranges.

**Characteristics:**

* O(log n) complexity for point lookups
* O(log n + k) for range queries (k = result size)
* Requires BTree index on target field
* Supports ordering, ranges, and prefix matching

**Use Cases:**

* Range queries (greater than, less than, between)
* Ordered iteration
* Prefix matching on strings
* Pagination with cursors

```rust
// BTree index access (Rust example)
collection.find_by_age_range(18..=65)     // Range query
collection.find_by_name_prefix("John")    // Prefix match
collection.iter_by_created_at()           // Ordered iteration
```

### 2.5 Full-Text Index Access (Phase 2)

Token-based text search.

**Characteristics:**

* Tokenizes text fields
* Exact token matching (no ranking in Phase 2)
* Requires FTS index on target field

**Use Cases:**

* Keyword search
* Tag matching
* Simple text queries

```rust
// Full-text access (Rust example)
collection.search_content("rust database")  // Token search
```

---

## 3. Index Declaration

### 3.1 Declaration API

Indexes are declared via typed API calls, not configuration files or DSL strings.

```rust
// Rust index declaration
db.define_index::<User>()
    .on(|u| &u.email)
    .hash()
    .unique()
    .build()?;

db.define_index::<User>()
    .on(|u| &u.created_at)
    .btree()
    .build()?;

db.define_index::<Post>()
    .on(|p| &p.author_id)
    .hash()
    .build()?;
```

### 3.2 Index Types

| Type | Declaration | Use Case |
|------|-------------|----------|
| Hash | `.hash()` | Equality lookups |
| BTree | `.btree()` | Ranges, ordering |
| Unique | `.unique()` | Enforce uniqueness |
| Composite | `.on(\|x\| (&x.a, &x.b))` | Multi-field indexes |

### 3.3 Index Constraints

* Index names are internal; users **MUST NOT** reference them.
* Indexes are automatically maintained on entity mutations.
* Index state **MUST** be derivable from entity data.
* Index corruption **MUST NOT** corrupt entity data.

---

## 4. Access Path Selection Algorithm

### 4.1 Selection Priority

When multiple access paths are available, the engine selects in this order:

1. **Primary key lookup** — If entity ID is known
2. **Unique hash index** — If equality on unique indexed field
3. **Non-unique hash index** — If equality on indexed field
4. **BTree index (point)** — If equality on BTree-indexed field
5. **BTree index (range)** — If range predicate on indexed field
6. **Full scan** — If no suitable index exists

### 4.2 Selection Rules

```
FUNCTION select_access_path(collection, access_request):
    IF access_request.has_entity_id:
        RETURN PrimaryKeyAccess(entity_id)
    
    FOR each predicate IN access_request.predicates:
        IF predicate.is_equality:
            IF hash_index_exists(collection, predicate.field):
                RETURN HashIndexAccess(index, predicate.value)
            ELSE IF btree_index_exists(collection, predicate.field):
                RETURN BTreePointAccess(index, predicate.value)
        
        ELSE IF predicate.is_range:
            IF btree_index_exists(collection, predicate.field):
                RETURN BTreeRangeAccess(index, predicate.range)
    
    RETURN FullScan(collection)
```

### 4.3 Composite Index Selection

For composite indexes on `(field_a, field_b)`:

* Prefix matching: Can use index if `field_a` predicate exists
* Full matching: Can use index if both `field_a` AND `field_b` predicates exist
* Suffix only: **Cannot** use index if only `field_b` predicate exists

---

## 5. Explicit Access API

### 5.1 Scan vs Indexed Distinction

The API **MUST** make the distinction between scans and indexed access explicit.

```rust
// EXPLICIT indexed access (preferred)
let user = users.get(user_id)?;                    // Primary key
let user = users.find_by_email("x@y.com")?;        // Hash index
let posts = posts.find_by_author(user_id);         // Index lookup

// EXPLICIT scan (requires opt-in)
let all_users = users.scan();                      // Full scan
let filtered = users.scan().filter(|u| u.age > 21); // Scan + filter
```

### 5.2 Access Path Hints

For advanced use cases, users **MAY** provide hints:

```rust
// Hint: prefer specific index (engine may ignore)
users.find_by_status("active")
    .hint(IndexHint::Prefer("status_idx"))
    
// Hint: force scan even if index exists
users.scan()
    .hint(AccessHint::ForceScan)
```

Hints are **advisory only**; the engine **MAY** ignore them.

---

## 6. Scan Safety

### 6.1 Telemetry Requirements

The engine **MUST** expose telemetry for access path usage:

```rust
pub struct AccessPathMetrics {
    pub path_type: AccessPathType,
    pub collection: String,
    pub index_name: Option<String>,
    pub entities_examined: u64,
    pub entities_returned: u64,
    pub duration_us: u64,
}

pub enum AccessPathType {
    PrimaryKey,
    HashIndex,
    BTreeIndex,
    FullScan,
}
```

### 6.2 Scan Detection

Applications **MUST** be able to detect when full scans occur:

```rust
// Telemetry callback
db.on_access_path(|metrics| {
    if metrics.path_type == AccessPathType::FullScan {
        log::warn!("Full scan on {}: {} entities", 
            metrics.collection, metrics.entities_examined);
    }
});
```

### 6.3 Scan Prevention

Configuration **MAY** forbid scans in production mode:

```rust
let config = DatabaseConfig::builder()
    .scan_policy(ScanPolicy::Forbid)  // Scans return error
    .build();

// Alternative policies
ScanPolicy::Allow           // Default: scans permitted
ScanPolicy::Warn            // Log warning on scan
ScanPolicy::Forbid          // Return error on scan attempt
ScanPolicy::ForbidUnbounded // Allow only bounded scans
```

### 6.4 Bounded Scans

To prevent unbounded iteration, scans **SHOULD** support limits:

```rust
// Bounded scan (safe)
let recent = posts.scan()
    .order_by(|p| p.created_at)
    .take(100);

// Unbounded scan (dangerous in production)
let all = posts.scan().collect::<Vec<_>>();
```

---

## 7. Index Maintenance

### 7.1 Transactional Updates

Index updates **MUST** be atomic with entity mutations:

* Index entries are updated within the same transaction.
* If transaction aborts, index changes are rolled back.
* Index state is always consistent with entity data.

### 7.2 Write Path

```
FUNCTION put_entity(txn, entity):
    old_entity = get_current(entity.id)
    
    // Update indexes atomically
    FOR each index ON collection:
        IF old_entity EXISTS:
            index.remove(old_entity)
        index.insert(entity)
    
    // Write entity
    write_to_segment(entity)
    write_to_wal(PUT, entity)
```

### 7.3 Index Rebuild

Indexes **MUST** be rebuildable from entity data:

```rust
// Rebuild specific index
db.rebuild_index::<User>("email_idx")?;

// Rebuild all indexes for collection
db.rebuild_all_indexes::<User>()?;
```

Rebuild is idempotent and produces identical results.

---

## 8. Determinism Requirements

### 8.1 Selection Determinism

Given identical:

* Schema (entity types and fields)
* Index definitions
* Access request (predicates and hints)

The access path selection **MUST** be deterministic and reproducible.

### 8.2 Result Ordering

* Hash index results have **no guaranteed order**.
* BTree index results are ordered by index key.
* Full scan results have **no guaranteed order** unless explicitly sorted.
* Primary key access returns single result (ordering N/A).

### 8.3 Cross-Platform Consistency

Access path selection **MUST** produce identical results across:

* Native platforms (Windows, macOS, Linux)
* Web platform (WASM)
* All language bindings (Rust, Dart, Python)

---

## 9. Performance Characteristics

### 9.1 Complexity Summary

| Access Path | Lookup | Range | Insert | Delete |
|-------------|--------|-------|--------|--------|
| Primary Key | O(1) | N/A | O(1) | O(1) |
| Hash Index | O(1) | N/A | O(1) | O(1) |
| BTree Index | O(log n) | O(log n + k) | O(log n) | O(log n) |
| Full Scan | O(n) | O(n) | N/A | N/A |

### 9.2 Memory Overhead

| Index Type | Memory per Entry |
|------------|-----------------|
| Hash Index | ~40-80 bytes |
| BTree Index | ~48-96 bytes |
| Primary Index | ~32-64 bytes |

### 9.3 Trade-offs

* More indexes → faster reads, slower writes
* Hash indexes → fastest equality, no ordering
* BTree indexes → flexible queries, higher memory

---

## 10. Rust Type Definitions

```rust
/// Access path types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessPath {
    /// Direct entity ID lookup.
    PrimaryKey { entity_id: EntityId },
    
    /// Hash index equality lookup.
    HashIndex { 
        index_name: String, 
        key: IndexKey,
    },
    
    /// BTree index point or range lookup.
    BTreeIndex {
        index_name: String,
        range: IndexRange,
    },
    
    /// Full collection scan.
    FullScan {
        collection: String,
        limit: Option<usize>,
    },
}

/// Index range for BTree access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexRange {
    /// Single value lookup.
    Point(IndexKey),
    
    /// Range with inclusive/exclusive bounds.
    Range {
        start: Bound<IndexKey>,
        end: Bound<IndexKey>,
    },
    
    /// Prefix match (strings only).
    Prefix(String),
}

/// Scan policy configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScanPolicy {
    /// Scans are allowed (default).
    #[default]
    Allow,
    
    /// Log warning on scan.
    Warn,
    
    /// Return error on scan attempt.
    Forbid,
    
    /// Allow only bounded scans (with limit).
    ForbidUnbounded,
}

/// Access path selection metrics.
#[derive(Debug, Clone)]
pub struct AccessMetrics {
    pub path: AccessPath,
    pub entities_examined: u64,
    pub entities_returned: u64,
    pub duration: std::time::Duration,
}
```

---

## 11. Invariants

### 11.1 Correctness Invariants

* Index usage **MUST NOT** change query results.
* Index absence **MUST NOT** cause query failure (fallback to scan).
* Index corruption **MUST NOT** corrupt entity data.
* Index rebuild **MUST** produce identical lookup results.

### 11.2 Safety Invariants

* Full scans **MUST** be detectable via telemetry.
* Full scans **MUST** be preventable via configuration.
* Unbounded scans **SHOULD** be avoided in production.

### 11.3 Determinism Invariants

* Access path selection **MUST** be deterministic.
* Same inputs **MUST** produce same access path.
* Cross-platform behavior **MUST** be identical.

---

## 12. References

* [architecture.md](architecture.md) — Section 11 (Indexing and Access Paths)
* [invariants.md](invariants.md) — Section 6 (Index Invariants)
* [transactions.md](transactions.md) — Atomic index updates
* [AGENTS.md](../AGENTS.md) — No SQL/DSL constraint
