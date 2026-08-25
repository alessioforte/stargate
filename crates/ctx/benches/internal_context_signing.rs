use std::hint::black_box;
use std::sync::Arc;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use ctx::{
    Actor, ActorType, Authentication, AuthenticationKind, ContextSigner, DispatchContext,
    DispatchKind, IssueRequest, Organization, RequestContext, RouteContext, SignerConfig,
    StargateContext, VERSION,
};
use rayon::ThreadPoolBuilder;
use rayon::prelude::*;

const PRIVATE_KEY: &[u8] = include_bytes!("../tests/fixtures/private.pem");

fn signer() -> Arc<ContextSigner> {
    Arc::new(
        ContextSigner::from_rsa_pem(
            SignerConfig::new(
                "https://auth.example.com/internal-context",
                "stargate-internal-benchmark",
                30,
            )
            .unwrap(),
            PRIVATE_KEY,
        )
        .unwrap(),
    )
}

fn request() -> Arc<IssueRequest> {
    Arc::new(IssueRequest {
        audience: "urn:stargate:service:orders".to_owned(),
        subject: Some("01JZ000000000000000000000A".to_owned()),
        context: StargateContext {
            v: VERSION,
            actor: Actor {
                actor_type: ActorType::User,
            },
            authentication: Authentication {
                kind: AuthenticationKind::Jwt,
                sid: Some("01JZ000000000000000000000B".to_owned()),
                auth_time: Some(1_784_473_000),
            },
            organization: Some(Organization {
                id: "01JZ000000000000000000000Z".to_owned(),
                role: Some("admin".to_owned()),
            }),
            request: RequestContext {
                id: "01JZ000000000000000000000R".to_owned(),
                trace_id: Some("4bf92f3577b34da6a3ce929d0e0e4736".to_owned()),
                method: "POST".to_owned(),
                path: "/v1/orders".to_owned(),
                original_path: "/api/orders".to_owned(),
                client_ip: Some("203.0.113.10".to_owned()),
                user_agent: Some("stargate-benchmark/1.0".to_owned()),
            },
            route: RouteContext {
                router: "orders-write".to_owned(),
                service: "orders".to_owned(),
                policy_revision: Some(
                    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        .to_owned(),
                ),
            },
            dispatch: DispatchContext {
                kind: DispatchKind::Primary,
                attempt: 1,
            },
        },
    })
}

fn benchmark_signing(criterion: &mut Criterion) {
    let signer = signer();
    let request = request();
    let sample_token_bytes = signer.issue(&request).unwrap().compact().len();
    eprintln!("internal_context_sample_token_bytes={sample_token_bytes}");

    let mut group = criterion.benchmark_group("internal_context_signing");
    group.throughput(Throughput::Elements(1));
    group.bench_function("single", |bencher| {
        bencher.iter(|| black_box(signer.issue(black_box(&request)).unwrap()))
    });

    for workers in [2_usize, 4] {
        let pool = ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap();
        let batch = workers * 16;
        group.throughput(Throughput::Elements(batch as u64));
        group.bench_with_input(
            BenchmarkId::new("parallel", format!("{workers}_workers")),
            &batch,
            |bencher, &batch| {
                bencher.iter(|| {
                    black_box(pool.install(|| {
                        (0..batch)
                            .into_par_iter()
                            .map(|_| signer.issue(&request).unwrap().compact().len())
                            .sum::<usize>()
                    }))
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_signing);
criterion_main!(benches);
