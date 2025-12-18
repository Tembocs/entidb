//! CBOR codec benchmarks.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use entidb_codec::{from_cbor, to_canonical_cbor, CanonicalEncoder, Value};

/// Create a simple map value.
fn simple_map() -> Value {
    Value::Map(vec![
        (Value::Text("name".into()), Value::Text("Alice".into())),
        (
            Value::Text("email".into()),
            Value::Text("alice@example.com".into()),
        ),
        (Value::Text("age".into()), Value::Integer(30)),
    ])
}

/// Create a complex nested value.
fn complex_value(depth: usize, width: usize) -> Value {
    if depth == 0 {
        Value::Text("leaf".into())
    } else {
        let children: Vec<(Value, Value)> = (0..width)
            .map(|i| {
                (
                    Value::Text(format!("key_{}", i)),
                    complex_value(depth - 1, width),
                )
            })
            .collect();
        Value::Map(children)
    }
}

/// Benchmark encoding simple values.
fn bench_encode_simple(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode");

    // Null
    group.bench_function("null", |b| {
        let value = Value::Null;
        b.iter(|| {
            let result = to_canonical_cbor(black_box(&value)).unwrap();
            black_box(result);
        });
    });

    // Integer
    group.bench_function("integer", |b| {
        let value = Value::Integer(42);
        b.iter(|| {
            let result = to_canonical_cbor(black_box(&value)).unwrap();
            black_box(result);
        });
    });

    // Text
    group.bench_function("text_short", |b| {
        let value = Value::Text("hello".into());
        b.iter(|| {
            let result = to_canonical_cbor(black_box(&value)).unwrap();
            black_box(result);
        });
    });

    // Bytes
    group.bench_function("bytes_256", |b| {
        let value = Value::Bytes(vec![0u8; 256]);
        b.iter(|| {
            let result = to_canonical_cbor(black_box(&value)).unwrap();
            black_box(result);
        });
    });

    // Simple map
    group.bench_function("map_simple", |b| {
        let value = simple_map();
        b.iter(|| {
            let result = to_canonical_cbor(black_box(&value)).unwrap();
            black_box(result);
        });
    });

    group.finish();
}

/// Benchmark encoding with varying sizes.
fn bench_encode_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode_size");

    for size in [64, 256, 1024, 4096, 16384].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let value = Value::Bytes(vec![0u8; size]);
            b.iter(|| {
                let result = to_canonical_cbor(black_box(&value)).unwrap();
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark decoding.
fn bench_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode");

    // Simple map
    group.bench_function("map_simple", |b| {
        let value = simple_map();
        let encoded = to_canonical_cbor(&value).unwrap();

        b.iter(|| {
            let result: Value = from_cbor(black_box(&encoded)).unwrap();
            black_box(result);
        });
    });

    // Bytes
    for size in [256, 1024, 4096].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::new("bytes", size), size, |b, &size| {
            let value = Value::Bytes(vec![0u8; size]);
            let encoded = to_canonical_cbor(&value).unwrap();

            b.iter(|| {
                let result: Value = from_cbor(black_box(&encoded)).unwrap();
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark roundtrip (encode + decode).
fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("roundtrip");

    group.bench_function("simple_map", |b| {
        let value = simple_map();

        b.iter(|| {
            let encoded = to_canonical_cbor(black_box(&value)).unwrap();
            let decoded: Value = from_cbor(&encoded).unwrap();
            black_box(decoded);
        });
    });

    group.bench_function("complex_depth3_width5", |b| {
        let value = complex_value(3, 5);

        b.iter(|| {
            let encoded = to_canonical_cbor(black_box(&value)).unwrap();
            let decoded: Value = from_cbor(&encoded).unwrap();
            black_box(decoded);
        });
    });

    group.finish();
}

/// Benchmark encoder reuse.
fn bench_encoder_reuse(c: &mut Criterion) {
    c.bench_function("encoder_reuse_100", |b| {
        let values: Vec<_> = (0..100).map(|i| Value::Integer(i)).collect();

        b.iter(|| {
            let mut encoder = CanonicalEncoder::new();
            for value in &values {
                encoder.encode(black_box(value)).unwrap();
            }
            black_box(encoder.into_bytes());
        });
    });
}

criterion_group!(
    benches,
    bench_encode_simple,
    bench_encode_size,
    bench_decode,
    bench_roundtrip,
    bench_encoder_reuse,
);

criterion_main!(benches);
