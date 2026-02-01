//! Throughput benchmarks for procwire-client.

use criterion::{criterion_group, criterion_main, Criterion};

fn wire_format_benchmark(c: &mut Criterion) {
    // TODO: Add benchmarks in TASK-01
    c.bench_function("header_encode", |b| {
        b.iter(|| {
            // Placeholder
        })
    });
}

criterion_group!(benches, wire_format_benchmark);
criterion_main!(benches);
