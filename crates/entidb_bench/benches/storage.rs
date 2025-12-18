//! Storage backend benchmarks.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use entidb_storage::{FileBackend, InMemoryBackend, StorageBackend};
use std::io::Write;
use tempfile::TempDir;

/// Create random data of given size.
fn random_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 256) as u8).collect()
}

/// Benchmark InMemoryBackend append operations.
fn bench_inmemory_append(c: &mut Criterion) {
    let mut group = c.benchmark_group("inmemory_append");

    for size in [64, 256, 1024, 4096].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let mut backend = InMemoryBackend::new();
            let data = random_data(size);

            b.iter(|| {
                let offset = backend.append(black_box(&data)).unwrap();
                black_box(offset);
            });
        });
    }

    group.finish();
}

/// Benchmark InMemoryBackend read operations.
fn bench_inmemory_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("inmemory_read");

    for size in [64, 256, 1024, 4096].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let mut backend = InMemoryBackend::new();
            let data = random_data(size);

            // Write data first
            let offset = backend.append(&data).unwrap();

            b.iter(|| {
                let result = backend.read_at(black_box(offset), black_box(size)).unwrap();
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark FileBackend append operations.
fn bench_file_append(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_append");

    // Use larger sample size for file operations
    group.sample_size(50);

    for size in [256, 1024, 4096].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let temp_dir = TempDir::new().unwrap();
            let path = temp_dir.path().join("bench.dat");

            // Create the file
            std::fs::File::create(&path).unwrap();
            let mut backend = FileBackend::open(&path).unwrap();
            let data = random_data(size);

            b.iter(|| {
                let offset = backend.append(black_box(&data)).unwrap();
                black_box(offset);
            });
        });
    }

    group.finish();
}

/// Benchmark FileBackend read operations.
fn bench_file_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_read");

    // Use larger sample size for file operations
    group.sample_size(50);

    for size in [256, 1024, 4096].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let temp_dir = TempDir::new().unwrap();
            let path = temp_dir.path().join("bench.dat");

            // Create and populate the file
            let mut file = std::fs::File::create(&path).unwrap();
            let data = random_data(size);
            file.write_all(&data).unwrap();
            file.sync_all().unwrap();
            drop(file);

            let backend = FileBackend::open(&path).unwrap();

            b.iter(|| {
                let result = backend.read_at(black_box(0), black_box(size)).unwrap();
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark FileBackend flush operations.
fn bench_file_flush(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_flush");
    group.sample_size(20); // Flush is slow

    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("bench.dat");
    std::fs::File::create(&path).unwrap();
    let mut backend = FileBackend::open(&path).unwrap();
    let data = random_data(1024);

    group.bench_function("after_1kb_write", |b| {
        b.iter(|| {
            backend.append(&data).unwrap();
            backend.flush().unwrap();
        });
    });

    group.finish();
}

/// Benchmark sequential append pattern (like WAL writes).
fn bench_sequential_append(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_append");
    group.sample_size(20);

    // InMemory - 1000 small records
    group.bench_function("inmemory_1000x64", |b| {
        let data = random_data(64);

        b.iter(|| {
            let mut backend = InMemoryBackend::new();
            for _ in 0..1000 {
                backend.append(black_box(&data)).unwrap();
            }
            let _ = black_box(backend.size());
        });
    });

    // File - 100 small records
    group.bench_function("file_100x64", |b| {
        let data = random_data(64);

        b.iter(|| {
            let temp_dir = TempDir::new().unwrap();
            let path = temp_dir.path().join("bench.dat");
            std::fs::File::create(&path).unwrap();
            let mut backend = FileBackend::open(&path).unwrap();

            for _ in 0..100 {
                backend.append(black_box(&data)).unwrap();
            }
            backend.flush().unwrap();
            let _ = black_box(backend.size());
        });
    });

    group.finish();
}

/// Benchmark random read pattern (like segment reads).
fn bench_random_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_read");

    // Prepare backend with many records
    let record_size = 256;
    let record_count = 1000;

    group.bench_function("inmemory_1000_records", |b| {
        let mut backend = InMemoryBackend::new();
        let data = random_data(record_size);

        let mut offsets = Vec::new();
        for _ in 0..record_count {
            offsets.push(backend.append(&data).unwrap());
        }

        let mut idx = 0;
        b.iter(|| {
            // Read records in pseudo-random order
            let offset = offsets[(idx * 7) % record_count];
            let result = backend.read_at(black_box(offset), black_box(record_size)).unwrap();
            idx = (idx + 1) % record_count;
            black_box(result);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_inmemory_append,
    bench_inmemory_read,
    bench_file_append,
    bench_file_read,
    bench_file_flush,
    bench_sequential_append,
    bench_random_read,
);

criterion_main!(benches);
