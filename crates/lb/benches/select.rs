use criterion::{Criterion, criterion_group, criterion_main};
use lb::{BaseLoadBalancer, IpHash, LoadBalancer, Random, RequestContext, RoundRobin, Upstream};
use std::hint::black_box;

/// Build `total` upstreams, the first `down` of which have an open circuit so
/// `select` must probe past them.
fn make_upstreams(total: usize, down: usize) -> Vec<Upstream> {
    (0..total)
        .map(|i| {
            // threshold 1 + long cooldown: one failure keeps it unavailable.
            let upstream =
                Upstream::with_circuit_breaker(format!("http://10.0.0.{i}:8080"), None, 1, 3600);
            if i < down {
                upstream.circuit_breaker.record_failure();
            }
            upstream
        })
        .collect()
}

fn ctx() -> RequestContext<'static> {
    RequestContext {
        client_ip: "203.0.113.42",
        path: "/api/resource",
        method: "GET",
        key: None,
    }
}

fn bench_select(c: &mut Criterion) {
    let mut group = c.benchmark_group("select");

    // all healthy: best case, first probe hits. half down: probe must skip
    // unavailable upstreams before finding a live one.
    for (label, total, down) in [("8_all_healthy", 8usize, 0usize), ("8_half_down", 8, 4)] {
        let request = ctx();

        let round_robin = BaseLoadBalancer::new(RoundRobin::new(), make_upstreams(total, down));
        group.bench_function(format!("round_robin/{label}"), |b| {
            b.iter(|| black_box(round_robin.select(black_box(&request))))
        });

        let random = BaseLoadBalancer::new(Random::new(), make_upstreams(total, down));
        group.bench_function(format!("random/{label}"), |b| {
            b.iter(|| black_box(random.select(black_box(&request))))
        });

        let ip_hash = BaseLoadBalancer::new(IpHash::new(), make_upstreams(total, down));
        group.bench_function(format!("ip_hash/{label}"), |b| {
            b.iter(|| black_box(ip_hash.select(black_box(&request))))
        });
    }

    group.finish();
}

criterion_group!(benches, bench_select);
criterion_main!(benches);
