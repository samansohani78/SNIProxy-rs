//! Connection pool operation benchmarks
//!
//! Benchmarks the performance improvements from Phase 1 DashMap migration

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use dashmap::DashMap;
use std::collections::HashMap;
use std::hint::black_box;
use std::sync::{Arc, Mutex};

/// Benchmark DashMap vs Mutex<HashMap> for concurrent access patterns
fn dashmap_vs_mutex_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_access");

    // DashMap (lock-free)
    group.bench_function("dashmap_insert", |b| {
        let map = Arc::new(DashMap::new());
        b.iter(|| {
            for i in 0..100 {
                let key = format!("host-{}", i);
                map.insert(key, vec![i as u8; 10]);
                black_box(&map);
            }
        });
    });

    // Mutex<HashMap> (locking)
    group.bench_function("mutex_hashmap_insert", |b| {
        let map = Arc::new(Mutex::new(HashMap::new()));
        b.iter(|| {
            for i in 0..100 {
                let key = format!("host-{}", i);
                let mut m = map.lock().unwrap();
                m.insert(key, vec![i as u8; 10]);
                drop(m); // Release lock
                black_box(&map);
            }
        });
    });

    // DashMap read
    group.bench_function("dashmap_read", |b| {
        let map = Arc::new(DashMap::new());
        for i in 0..100 {
            map.insert(format!("host-{}", i), vec![i as u8; 10]);
        }

        b.iter(|| {
            for i in 0..100 {
                let key = format!("host-{}", i);
                let _val = map.get(&key);
                black_box(&map);
            }
        });
    });

    // Mutex<HashMap> read
    group.bench_function("mutex_hashmap_read", |b| {
        let map = Arc::new(Mutex::new(HashMap::new()));
        {
            let mut m = map.lock().unwrap();
            for i in 0..100 {
                m.insert(format!("host-{}", i), vec![i as u8; 10]);
            }
        }

        b.iter(|| {
            for i in 0..100 {
                let key = format!("host-{}", i);
                let m = map.lock().unwrap();
                let _val = m.get(&key);
                drop(m); // Release lock
                black_box(&map);
            }
        });
    });

    group.finish();
}

/// Benchmark pool lookup patterns
fn pool_lookup_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("pool_lookup");

    for num_hosts in [10, 100, 1000] {
        // Populate DashMap
        let map = Arc::new(DashMap::new());
        for i in 0..num_hosts {
            map.insert(format!("host-{}", i), vec![i as u8; 10]);
        }

        group.bench_with_input(
            BenchmarkId::new("dashmap_get_mut", num_hosts),
            &num_hosts,
            |b, &_num_hosts| {
                b.iter(|| {
                    let key = format!("host-{}", 42 % num_hosts);
                    if let Some(mut entry) = map.get_mut(&key) {
                        entry.push(1);
                        black_box(&entry);
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark entry API (or_default pattern)
fn entry_api_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("entry_api");

    // DashMap entry().or_default()
    group.bench_function("dashmap_entry_or_default", |b| {
        let map = Arc::new(DashMap::<String, Vec<u8>>::new());
        b.iter(|| {
            for i in 0..100 {
                let key = format!("host-{}", i % 10); // Reuse some keys
                map.entry(key).or_default();
                black_box(&map);
            }
        });
    });

    // Mutex<HashMap> entry().or_default()
    group.bench_function("mutex_entry_or_default", |b| {
        let map = Arc::new(Mutex::new(HashMap::<String, Vec<u8>>::new()));
        b.iter(|| {
            for i in 0..100 {
                let key = format!("host-{}", i % 10); // Reuse some keys
                let mut m = map.lock().unwrap();
                m.entry(key).or_default();
                drop(m);
                black_box(&map);
            }
        });
    });

    group.finish();
}

/// Benchmark iteration performance
fn iteration_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("iteration");

    // DashMap iteration
    group.bench_function("dashmap_iter_count", |b| {
        let map = Arc::new(DashMap::new());
        for i in 0..1000 {
            map.insert(format!("host-{}", i), vec![i as u8; 10]);
        }

        b.iter(|| {
            let count: usize = map.iter().map(|entry| entry.value().len()).sum();
            black_box(count);
        });
    });

    // Mutex<HashMap> iteration
    group.bench_function("mutex_iter_count", |b| {
        let map = Arc::new(Mutex::new(HashMap::new()));
        {
            let mut m = map.lock().unwrap();
            for i in 0..1000 {
                m.insert(format!("host-{}", i), vec![i as u8; 10]);
            }
        }

        b.iter(|| {
            let m = map.lock().unwrap();
            let count: usize = m.values().map(|v| v.len()).sum();
            black_box(count);
        });
    });

    group.finish();
}

/// Benchmark cleanup operations (retain pattern)
fn cleanup_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("cleanup");

    // DashMap retain
    group.bench_function("dashmap_retain", |b| {
        b.iter(|| {
            let map = Arc::new(DashMap::new());
            for i in 0..1000 {
                map.insert(format!("host-{}", i), i);
            }

            // Remove half the entries
            map.retain(|_, v| *v % 2 == 0);
            black_box(&map);
        });
    });

    // Mutex<HashMap> retain
    group.bench_function("mutex_retain", |b| {
        b.iter(|| {
            let map = Arc::new(Mutex::new(HashMap::new()));
            {
                let mut m = map.lock().unwrap();
                for i in 0..1000 {
                    m.insert(format!("host-{}", i), i);
                }
            }

            // Remove half the entries
            {
                let mut m = map.lock().unwrap();
                m.retain(|_, v| *v % 2 == 0);
            }
            black_box(&map);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    dashmap_vs_mutex_benchmark,
    pool_lookup_benchmark,
    entry_api_benchmark,
    iteration_benchmark,
    cleanup_benchmark
);
criterion_main!(benches);
