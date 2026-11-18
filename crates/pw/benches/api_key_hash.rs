use criterion::{Criterion, criterion_group, criterion_main};
use pw::hash_api_key;
use std::hint::black_box;

/// Hash cost of the API key format (`<type>_<env>_<43 base64url chars>`).
///
/// Every API key request pays this hash before the subject read it addresses.
/// The recorded request-path measurement puts one application store read at
/// 0.288 / 0.393 / 0.600 ms (docs/gateway-performance.md, "Request paths with
/// cluster backends"), so the hash is negligible next to the lookup and a
/// cache of cleartext keys buys nothing.
fn bench_hash(c: &mut Criterion) {
    let key = format!("secret_live_{}", "a".repeat(43));
    c.bench_function("api_key_hash/46_bytes", |b| {
        b.iter(|| black_box(hash_api_key(black_box(&key))))
    });
}

criterion_group!(benches, bench_hash);
criterion_main!(benches);
