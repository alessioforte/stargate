use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::time::Duration;
use store::memory::{MemoryStore, MemoryStoreConfig};
use store::{AtomicStore, Store};

fn build_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime for benchmarks")
}

fn bench_set_get(c: &mut Criterion) {
    let rt = build_runtime();
    let mut group = c.benchmark_group("memory_set_get");

    for payload_size in [32usize, 256, 4096] {
        let value = "x".repeat(payload_size);
        let store = MemoryStore::with_config(MemoryStoreConfig::high_performance());

        rt.block_on(async {
            store.set("hit_key", &value, None).await.unwrap();
        });

        group.throughput(Throughput::Bytes(payload_size as u64));

        group.bench_with_input(
            BenchmarkId::new("get_hit", payload_size),
            &payload_size,
            |b, _| {
                b.to_async(&rt).iter(|| async {
                    let got: Option<String> = store.get("hit_key").await.unwrap();
                    criterion::black_box(got);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("set_overwrite", payload_size),
            &payload_size,
            |b, _| {
                b.to_async(&rt).iter(|| async {
                    store.set("hit_key", &value, None).await.unwrap();
                });
            },
        );
    }

    let miss_store = MemoryStore::with_config(MemoryStoreConfig::high_performance());
    group.bench_function("get_miss", |b| {
        b.to_async(&rt).iter(|| async {
            let got: Option<String> = miss_store.get("missing").await.unwrap();
            criterion::black_box(got);
        });
    });

    group.finish();
}

fn bench_hash_ops(c: &mut Criterion) {
    let rt = build_runtime();
    let mut group = c.benchmark_group("memory_hash");

    let store = MemoryStore::with_config(MemoryStoreConfig::high_performance());
    let field_count = 128usize;

    rt.block_on(async {
        for i in 0..field_count {
            let field = format!("field:{i}");
            store.hset("users", &field, &i, None).await.unwrap();
        }
    });

    group.throughput(Throughput::Elements(field_count as u64));

    group.bench_function("hget_hit", |b| {
        b.to_async(&rt).iter(|| async {
            let got: Option<i32> = store.hget("users", "field:64").await.unwrap();
            criterion::black_box(got);
        });
    });

    group.bench_function("hkeys", |b| {
        b.to_async(&rt).iter(|| async {
            let keys = store.hkeys("users").await.unwrap();
            criterion::black_box(keys);
        });
    });

    group.bench_function("hgetall", |b| {
        b.to_async(&rt).iter(|| async {
            let all: std::collections::HashMap<String, i32> = store.hgetall("users").await.unwrap();
            criterion::black_box(all);
        });
    });

    group.finish();
}

fn bench_atomic_ops(c: &mut Criterion) {
    let rt = build_runtime();
    let mut group = c.benchmark_group("memory_atomic");

    let store = MemoryStore::with_config(MemoryStoreConfig::high_performance());
    rt.block_on(async {
        store.set_i64("counter", 0, None).await.unwrap();
    });

    group.bench_function("incr_i64", |b| {
        b.to_async(&rt).iter(|| async {
            let new_value = store.incr_i64("counter", 1, None).await.unwrap();
            criterion::black_box(new_value);
        });
    });

    group.bench_function("compare_and_swap_i64_fail", |b| {
        b.to_async(&rt).iter(|| async {
            // Keep failing on purpose to benchmark contention-free failure path.
            let res = store
                .compare_and_swap_i64("counter", -1, 0, None)
                .await
                .unwrap();
            criterion::black_box(res);
        });
    });

    group.finish();
}

fn bench_batch_ops(c: &mut Criterion) {
    let rt = build_runtime();
    let mut group = c.benchmark_group("memory_batch");

    let store = MemoryStore::with_config(MemoryStoreConfig::high_performance());
    let keys: Vec<String> = (0..128).map(|i| format!("k:{i}")).collect();
    let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();

    rt.block_on(async {
        for (i, key) in key_refs.iter().enumerate() {
            store.set(key, &i, None).await.unwrap();
        }
    });

    group.throughput(Throughput::Elements(key_refs.len() as u64));

    group.bench_function("batch_get_128", |b| {
        b.to_async(&rt).iter(|| async {
            let out: Vec<Option<usize>> = store.batch_get(&key_refs).await.unwrap();
            criterion::black_box(out);
        });
    });

    let value = 42usize;
    let operations: Vec<(&str, &usize, Option<u64>)> =
        key_refs.iter().map(|key| (*key, &value, None)).collect();

    group.bench_function("batch_set_128", |b| {
        b.to_async(&rt).iter(|| async {
            let out = store.batch_set(&operations).await.unwrap();
            criterion::black_box(out);
        });
    });

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .sample_size(30)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2));
    targets = bench_set_get, bench_hash_ops, bench_atomic_ops, bench_batch_ops
);
criterion_main!(benches);
