//! Throughput benchmarks for buffer copy operations
//!
//! Benchmarks the performance improvements from Phase 1 buffer size increases

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

/// Benchmark buffer allocation and usage for different sizes
fn buffer_allocation_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_allocation");

    for size in [8192, 16384, 32768] {
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::new("allocate_and_use", size),
            &size,
            |b, &size| {
                b.iter(|| {
                    let buf = vec![0u8; size];
                    black_box(buf);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark simulated copy operations with different buffer sizes
fn copy_throughput_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("copy_throughput");

    // Simulate 1MB of data transfer
    const TOTAL_DATA: usize = 1024 * 1024;

    for buffer_size in [8192, 16384, 32768] {
        group.throughput(Throughput::Bytes(TOTAL_DATA as u64));
        group.bench_with_input(
            BenchmarkId::new("copy_operations", buffer_size),
            &buffer_size,
            |b, &buffer_size| {
                b.iter(|| {
                    let mut total_copied = 0;
                    let mut buf = vec![0u8; buffer_size];

                    // Simulate copies until we've transferred all data
                    while total_copied < TOTAL_DATA {
                        // Simulate reading into buffer
                        black_box(&mut buf[..]);
                        total_copied += buffer_size;
                    }

                    black_box(total_copied);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark syscall reduction (simulated)
fn syscall_reduction_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("syscall_reduction");

    // Transfer 1MB of data
    const TOTAL_DATA: usize = 1024 * 1024;

    for buffer_size in [8192, 16384, 32768] {
        let syscalls_needed = TOTAL_DATA / buffer_size;

        group.bench_with_input(
            BenchmarkId::new("syscalls_for_1mb", buffer_size),
            &buffer_size,
            |b, &buffer_size| {
                b.iter(|| {
                    let mut total = 0;
                    let buf = vec![0u8; buffer_size];

                    // Count syscalls needed
                    for _ in 0..syscalls_needed {
                        black_box(&buf[..]);
                        total += buffer_size;
                    }

                    black_box(total);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark bidirectional copy simulation
fn bidirectional_copy_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("bidirectional_copy");

    for buffer_size in [8192, 16384, 32768] {
        group.throughput(Throughput::Bytes(buffer_size as u64));
        group.bench_with_input(
            BenchmarkId::new("both_directions", buffer_size),
            &buffer_size,
            |b, &buffer_size| {
                b.iter(|| {
                    let buf_client = vec![0u8; buffer_size];
                    let buf_server = vec![0u8; buffer_size];

                    // Simulate client->server
                    black_box(&buf_client[..]);

                    // Simulate server->client
                    black_box(&buf_server[..]);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    buffer_allocation_benchmark,
    copy_throughput_benchmark,
    syscall_reduction_benchmark,
    bidirectional_copy_benchmark
);
criterion_main!(benches);
