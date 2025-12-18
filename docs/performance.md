# EntiDB Performance Profile

This document summarizes benchmark results and identifies optimization opportunities.

## Benchmark Summary

All benchmarks run on release build with optimizations enabled.

### CBOR Codec Performance

| Operation | Size | Time | Throughput |
|-----------|------|------|------------|
| Encode null | 1 byte | 135-143 ns | - |
| Encode integer | 1-5 bytes | 128-129 ns | - |
| Encode text (short) | 6 bytes | 130-131 ns | - |
| Encode bytes | 256 bytes | 277-278 ns | 877-881 MiB/s |
| Encode bytes | 1024 bytes | 294-295 ns | 3.24 GiB/s |
| Encode bytes | 4096 bytes | 378-390 ns | 9.8-10.1 GiB/s |
| Encode bytes | 16384 bytes | 1.33-1.35 µs | 11.3-11.5 GiB/s |
| Encode map (simple) | ~50 bytes | 1.18-1.24 µs | - |
| Decode map (simple) | ~50 bytes | 1.25-1.38 µs | - |
| Decode bytes | 256 bytes | 170-171 ns | 1.39-1.40 GiB/s |
| Decode bytes | 1024 bytes | 188-190 ns | 5.02-5.07 GiB/s |
| Decode bytes | 4096 bytes | 243-249 ns | 15.3-15.7 GiB/s |
| Roundtrip complex (d=3, w=5) | ~5KB | 102-103 µs | - |

**Analysis:**
- CBOR codec is very efficient, achieving 10+ GiB/s for large payloads
- Small value encoding has ~130ns fixed overhead
- Map encoding/decoding is ~1.2µs due to key sorting for canonical CBOR

### Database Operations

| Operation | Parameters | Time | Throughput |
|-----------|------------|------|------------|
| **Write (single)** | 64 bytes | 4.28-4.31 µs | 14.2 MiB/s |
| Write (single) | 256 bytes | 6.38-6.43 µs | 38.0 MiB/s |
| Write (single) | 1024 bytes | 15.0-15.8 µs | 62-65 MiB/s |
| Write (single) | 4096 bytes | 48.2-48.8 µs | 80.0-81.0 MiB/s |
| **Write (batch)** | 10 entities | 50.1-50.6 µs | 198-200K elem/s |
| Write (batch) | 100 entities | 479-483 µs | 207-208K elem/s |
| Write (batch) | 1000 entities | 5.00-5.04 ms | 198-200K elem/s |
| **Read (single)** | 64 bytes | 983-988 ns | 61.8-62.1 MiB/s |
| Read (single) | 256 bytes | 1.84-1.84 µs | 132-133 MiB/s |
| Read (single) | 1024 bytes | 5.32-5.35 µs | 183-184 MiB/s |
| Read (single) | 4096 bytes | 19.1-19.2 µs | 204 MiB/s |
| **Read (populated)** | 100 entities | 1.87-1.88 µs | - |
| Read (populated) | 1000 entities | 1.86-1.88 µs | - |
| Read (populated) | 10000 entities | 2.03-2.04 µs | - |
| **Transaction overhead** | empty | 1.14 µs | - |
| **Delete** | single | 2.60-2.66 µs | - |
| **List** | 100 entities | 91.5-92.6 µs | 1.08-1.09 Melem/s |
| List | 1000 entities | 904-907 µs | 1.10 Melem/s |
| List | 10000 entities | 9.43-9.50 ms | 1.05-1.06 Melem/s |

**Analysis:**
- Read performance scales well with database size (~2µs for 10K entities)
- Batch writes are efficient (~200K entities/second)
- Transaction overhead is minimal (~1.1µs)
- List operations have consistent throughput (~1M elements/s)

### Storage Backend Performance

| Operation | Backend | Size | Time | Throughput |
|-----------|---------|------|------|------------|
| **Append** | In-Memory | 64 bytes | 81-85 ns | 716-755 MiB/s |
| Append | In-Memory | 256 bytes | 234-245 ns | 997 MiB/s - 1.02 GiB/s |
| Append | In-Memory | 1024 bytes | 825-875 ns | 1.09-1.16 GiB/s |
| Append | In-Memory | 4096 bytes | 3.39-3.70 µs | 1.03-1.12 GiB/s |
| Append | File | 256 bytes | 10.1-10.4 µs | 23.5-24.2 MiB/s |
| Append | File | 1024 bytes | 11.2-11.5 µs | 85.3-87.6 MiB/s |
| Append | File | 4096 bytes | 14.7-16.6 µs | 235-266 MiB/s |
| **Read** | In-Memory | 64 bytes | 134-135 ns | 452-455 MiB/s |
| Read | In-Memory | 256 bytes | 142-143 ns | 1.67 GiB/s |
| Read | In-Memory | 1024 bytes | 151 ns | 6.30-6.33 GiB/s |
| Read | In-Memory | 4096 bytes | 216-222 ns | 17.2-17.6 GiB/s |
| Read | File | 256 bytes | 5.21-5.31 µs | 46.0-46.8 MiB/s |
| Read | File | 1024 bytes | 5.26-5.30 µs | 184-186 MiB/s |
| Read | File | 4096 bytes | 5.52-5.55 µs | 704-708 MiB/s |
| **Flush** | File | 1KB written | 11.8-12.2 µs | - |
| **Sequential Append** | In-Memory | 1000×64B | 38.2-38.3 µs | - |
| Sequential Append | File | 100×64B | 3.25-4.28 ms | - |
| **Random Read** | In-Memory | 1000 records | 159 ns | - |

**Analysis:**
- In-memory backend is extremely fast (17+ GiB/s reads)
- File I/O is the primary bottleneck for persistent operations
- File reads have ~5µs baseline overhead (OS syscall)
- Flush overhead is ~12µs

---

## Hot Paths Identified

### 1. Transaction Commit (Priority: High)

The transaction commit path is critical:
```
transaction() → put/delete operations → commit → WAL write → flush
```

**Current:** ~4-6µs per single write
**Bottleneck:** WAL encoding and flush to disk
**Optimization:** WAL batching, async flush option

### 2. Entity Lookup (Priority: High)

Reading entities is frequent:
```
get(collection, entity_id) → hash lookup → segment scan → decode
```

**Current:** ~1-2µs for small entities
**Bottleneck:** Segment record iteration
**Optimization:** In-memory entity index with offset pointers

### 3. CBOR Map Encoding (Priority: Medium)

Map encoding requires key sorting:
```
encode map → collect keys → sort by bytes → encode pairs
```

**Current:** ~1.2µs for simple maps
**Bottleneck:** Allocation + sorting
**Optimization:** Pre-sorted key insertion, arena allocation

### 4. List/Scan Operations (Priority: Medium)

Full collection scans:
```
list(collection) → iterate all segments → filter by collection → collect
```

**Current:** ~900µs for 1000 entities
**Bottleneck:** Memory allocation for result vector
**Optimization:** Iterator-based API, streaming results

---

## Optimization Recommendations

### Phase 1: Quick Wins

1. **Add `#[inline]` hints to hot paths**
   - CBOR encode/decode primitives
   - Entity ID comparison
   - Hash index lookup

2. **Use `SmallVec` for small allocations**
   - WAL record payloads < 128 bytes
   - Short text strings in CBOR

3. **Cache segment record offsets**
   - Maintain offset index during writes
   - Avoid re-scanning on reads

### Phase 2: Structural Improvements

1. **Implement async flush option**
   ```rust
   Config {
       sync_on_commit: false,  // Batch flushes
       flush_interval_ms: 100,
   }
   ```

2. **Add entity offset index**
   ```rust
   struct OffsetIndex {
       // (collection_id, entity_id) → segment offset
       map: HashMap<(u32, EntityId), u64>,
   }
   ```

3. **Implement iterator-based list**
   ```rust
   fn iter(&self, collection: CollectionId) -> impl Iterator<Item = (EntityId, &[u8])>
   ```

### Phase 3: Advanced Optimizations

1. **Memory-mapped segments**
   - Use mmap for read-heavy workloads
   - Lazy loading of segment data

2. **Parallel segment scanning**
   - Use rayon for multi-threaded scans
   - Partition work by segment

3. **Write-ahead log compression**
   - LZ4 compression for large payloads
   - Configurable threshold

---

## Performance Targets

| Operation | Current | Target | Improvement |
|-----------|---------|--------|-------------|
| Single write (256B) | 6.4µs | 3µs | 2x |
| Single read (256B) | 1.8µs | 500ns | 3.6x |
| Batch write (1000) | 5ms | 2ms | 2.5x |
| List (1000) | 900µs | 300µs | 3x |
| Transaction overhead | 1.1µs | 500ns | 2.2x |

---

## Benchmark Commands

```bash
# Full benchmark suite
cargo bench -p entidb_bench

# Specific benchmark
cargo bench -p entidb_bench -- single_write

# Generate HTML report
cargo bench -p entidb_bench -- --save-baseline main

# Compare against baseline
cargo bench -p entidb_bench -- --baseline main
```

---

## Appendix: System Information

Benchmarks should be run on consistent hardware. Document:
- CPU model and core count
- RAM speed and capacity
- Storage type (SSD/NVMe/HDD)
- OS and kernel version

Example:
```
CPU: Intel Core i7-12700K @ 3.6GHz
RAM: 32GB DDR5-4800
Storage: Samsung 980 Pro NVMe
OS: Windows 11 / Ubuntu 22.04
```
