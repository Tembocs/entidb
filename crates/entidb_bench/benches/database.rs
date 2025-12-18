//! Database operation benchmarks.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use entidb_core::{Database, EntityId};
use rand::Rng;

/// Generate random data of the specified size.
fn random_data(size: usize) -> Vec<u8> {
    let mut rng = rand::thread_rng();
    (0..size).map(|_| rng.gen()).collect()
}

/// Benchmark single entity writes.
fn bench_single_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_write");

    for size in [64, 256, 1024, 4096].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let db = Database::open_in_memory().unwrap();
            let collection = db.collection("bench");
            let data = random_data(size);

            b.iter(|| {
                let id = EntityId::new();
                db.transaction(|txn| {
                    txn.put(collection, id, black_box(data.clone()))?;
                    Ok(())
                })
                .unwrap();
            });
        });
    }
    group.finish();
}

/// Benchmark batch writes.
fn bench_batch_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_write");

    for batch_size in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*batch_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            batch_size,
            |b, &batch_size| {
                let db = Database::open_in_memory().unwrap();
                let collection = db.collection("bench");

                // Pre-generate data
                let entities: Vec<_> = (0..batch_size)
                    .map(|_| (EntityId::new(), random_data(256)))
                    .collect();

                b.iter(|| {
                    db.transaction(|txn| {
                        for (id, data) in &entities {
                            txn.put(collection, *id, black_box(data.clone()))?;
                        }
                        Ok(())
                    })
                    .unwrap();
                });
            },
        );
    }
    group.finish();
}

/// Benchmark single entity reads.
fn bench_single_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_read");

    for size in [64, 256, 1024, 4096].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let db = Database::open_in_memory().unwrap();
            let collection = db.collection("bench");
            let id = EntityId::new();
            let data = random_data(size);

            // Pre-populate
            db.transaction(|txn| {
                txn.put(collection, id, data)?;
                Ok(())
            })
            .unwrap();

            b.iter(|| {
                let result = db.get(collection, black_box(id)).unwrap();
                black_box(result);
            });
        });
    }
    group.finish();
}

/// Benchmark read from populated database.
fn bench_read_populated(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_populated");

    for entity_count in [100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(entity_count),
            entity_count,
            |b, &count| {
                let db = Database::open_in_memory().unwrap();
                let collection = db.collection("bench");

                // Pre-populate
                let mut ids = Vec::with_capacity(count);
                db.transaction(|txn| {
                    for _ in 0..count {
                        let id = EntityId::new();
                        txn.put(collection, id, random_data(256))?;
                        ids.push(id);
                    }
                    Ok(())
                })
                .unwrap();

                let mut rng = rand::thread_rng();

                b.iter(|| {
                    let idx = rng.gen_range(0..ids.len());
                    let result = db.get(collection, black_box(ids[idx])).unwrap();
                    black_box(result);
                });
            },
        );
    }
    group.finish();
}

/// Benchmark transaction overhead (empty transaction).
fn bench_transaction_overhead(c: &mut Criterion) {
    c.bench_function("transaction_overhead", |b| {
        let db = Database::open_in_memory().unwrap();

        b.iter(|| {
            db.transaction(|_txn| Ok(())).unwrap();
        });
    });
}

/// Benchmark entity deletion.
fn bench_delete(c: &mut Criterion) {
    c.bench_function("delete", |b| {
        let db = Database::open_in_memory().unwrap();
        let collection = db.collection("bench");

        b.iter_batched(
            || {
                // Setup: create an entity
                let id = EntityId::new();
                db.transaction(|txn| {
                    txn.put(collection, id, random_data(256))?;
                    Ok(())
                })
                .unwrap();
                id
            },
            |id| {
                // Benchmark: delete it
                db.transaction(|txn| {
                    txn.delete(collection, black_box(id))?;
                    Ok(())
                })
                .unwrap();
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

/// Benchmark list operation.
fn bench_list(c: &mut Criterion) {
    let mut group = c.benchmark_group("list");

    for entity_count in [100, 1000, 10000].iter() {
        group.throughput(Throughput::Elements(*entity_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(entity_count),
            entity_count,
            |b, &count| {
                let db = Database::open_in_memory().unwrap();
                let collection = db.collection("bench");

                // Pre-populate
                db.transaction(|txn| {
                    for _ in 0..count {
                        txn.put(collection, EntityId::new(), random_data(64))?;
                    }
                    Ok(())
                })
                .unwrap();

                b.iter(|| {
                    let result = db.list(black_box(collection)).unwrap();
                    black_box(result);
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_single_write,
    bench_batch_write,
    bench_single_read,
    bench_read_populated,
    bench_transaction_overhead,
    bench_delete,
    bench_list,
);

criterion_main!(benches);
