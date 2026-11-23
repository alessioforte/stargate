//! Per-path request baseline against disposable PostgreSQL and Redis.
//!
//! Unlike `src/gate/performance/` (memory store, CPU and bounded work), this
//! workload measures what each gateway path costs in state-store commands and
//! latency with the cluster backends. Run it through
//! `benches/scripts/benchmark_request_paths.py`, which starts the backends and
//! exports the environment; the results are recorded in
//! `docs/gateway-performance.md`.

use crate::{
    data::store::use_store,
    gate::Gate,
    iam::{
        auth::jwt as session_jwt,
        sess::{issuance::start_user_session, registry},
    },
};
use axum::{Extension, Router, body::Body};
use db::ent::{
    CredentialType, OAuthClient, Profile, TrustedAdminActor, TrustedAuditContext,
    TrustedAuditRequest, User,
};
use gate::{PolicySnapshot, cfg::Config};
use http::Request;
use http_body_util::BodyExt;
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use store::{RedisConnection, Store};
use tokio::task::JoinSet;

const AUDIENCE: &str = "urn:stargate:benchmark";
const CLIENT_ID: &str = "stargate-benchmark";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Credential {
    None,
    ApiKey,
    Session,
    Oauth,
}

struct Scenario {
    name: &'static str,
    path: &'static str,
    credential: Credential,
}

/// The paths of the baseline: four credentials and the two traffic policies.
/// Policy rows run anonymously so their figures are the policy's own cost.
/// `durable_quota` adds the calendar quota, which needs an authenticated
/// subject: its figures include the API key path.
const SCENARIOS: [Scenario; 7] = [
    Scenario {
        name: "anonymous",
        path: "/anonymous",
        credential: Credential::None,
    },
    Scenario {
        name: "api_key",
        path: "/api-key",
        credential: Credential::ApiKey,
    },
    Scenario {
        name: "session",
        path: "/session",
        credential: Credential::Session,
    },
    Scenario {
        name: "oauth",
        path: "/oauth",
        credential: Credential::Oauth,
    },
    Scenario {
        name: "rate_limit",
        path: "/rate-limit",
        credential: Credential::None,
    },
    Scenario {
        name: "quota",
        path: "/quota",
        credential: Credential::None,
    },
    Scenario {
        name: "durable_quota",
        path: "/durable-quota",
        credential: Credential::ApiKey,
    },
];

struct Fixtures {
    api_key: String,
    session_token: String,
    oauth_token: String,
}

impl Fixtures {
    async fn create() -> Self {
        let suffix = ulid::Ulid::generate().to_string().to_lowercase();
        // Audit producers for users and API keys accept admin control-plane work only.
        let context = TrustedAuditContext::admin_control_plane(
            TrustedAdminActor::admin("01JZ000000000000000000000A"),
            TrustedAuditRequest::from_http(ulid::Ulid::generate().to_string(), None, None, None),
        );
        let user: User = crate::data::db::create_user(
            Profile::new(
                format!("benchmark-{suffix}@example.test"),
                format!("benchmark-{suffix}"),
            ),
            CredentialType::Password,
            "benchmark-credential",
            None,
            context.clone(),
        )
        .await
        .unwrap();
        crate::data::db::create_oauth_client(
            OAuthClient::new(
                CLIENT_ID.to_string(),
                None,
                "Benchmark client".to_string(),
                None,
                "none".to_string(),
                vec!["authorization_code".to_string()],
                vec!["code".to_string()],
                vec!["https://client.example.test/callback".to_string()],
                vec!["openid".to_string()],
                vec![AUDIENCE.to_string()],
                json!({"first_party": true}),
            ),
            context.clone(),
        )
        .await
        .unwrap();

        let auth_time = chrono::Utc::now().timestamp() as usize;
        let issued = start_user_session(user.clone(), auth_time, None, vec!["pwd".to_string()])
            .await
            .unwrap();
        let claims = session_jwt::jwt_config()
            .validate_session_access_token(&issued.access_token)
            .unwrap();
        let sid = claims.sid.expect("session tokens carry sid");
        registry::register_session_client(&sid, &user.id, CLIENT_ID)
            .await
            .unwrap();

        let api_key = format!("benchmark-{suffix}");
        crate::data::db::create_user_api_key(
            &user.id,
            &pw::hash_api_key(&api_key),
            "benchmark",
            None,
            None,
            context,
        )
        .await
        .unwrap();

        let claims = oidc::claims::user_access_claims(
            &oidc::claims::UserClaimsProfile {
                id: user.id.clone(),
                email: user.email.clone(),
                given_name: user.given_name.clone(),
                family_name: user.family_name.clone(),
                nickname: user.nickname.clone(),
                picture: user.picture.clone(),
                locale: None,
            },
            CLIENT_ID,
            Some(&sid),
            None,
            Some(AUDIENCE.to_string()),
            chrono::Utc::now(),
            &oidc::authentication::Authentication {
                amr: vec![oidc::authentication::AMR_PASSWORD.to_string()],
                acr: oidc::authentication::ACR_SINGLE_FACTOR.to_string(),
            },
        );
        let oauth_token = session_jwt::jwt_config()
            .generate_oauth_access_token(claims)
            .unwrap();

        Self {
            api_key,
            session_token: issued.access_token,
            oauth_token,
        }
    }

    fn credential(&self, scenario: &Scenario) -> Option<(&'static str, String)> {
        match scenario.credential {
            Credential::None => None,
            Credential::ApiKey => Some(("x-api-key", self.api_key.clone())),
            Credential::Session => {
                Some(("authorization", format!("Bearer {}", self.session_token)))
            }
            Credential::Oauth => Some(("authorization", format!("Bearer {}", self.oauth_token))),
        }
    }
}

fn config() -> Config {
    let generous =
        json!({"strategy": "gcra", "params": {"max_burst": 10_000_000, "replenish_1_per": "1us"}});
    serde_json::from_value(json!({
        "schema": "stargate/v1",
        "ingress": {"limit": "ingress", "timeout": "250ms"},
        "limits": {
            "default": generous.clone(),
            "ingress": generous.clone(),
            "rate": generous,
            "quota": {"strategy": "quota_tracker", "params": {"limit": 1_000_000_000, "period": "hour"}},
            "durable-quota": {"strategy": "quota_tracker", "params": {"limit": 1_000_000_000, "period": "day", "timezone": "UTC"}},
        },
        "http": {
            "services": {"local": {"kind": "direct_response", "status": 200}},
            "policies": {
                "api-key": {"kind": "auth", "strategies": ["api_key"]},
                "session": {"kind": "auth", "strategies": ["jwt"]},
                "oauth": {"kind": "auth", "strategies": ["oauth"], "audience": AUDIENCE},
                "rate": {"kind": "rate_limit", "limit": "rate"},
                "quota": {"kind": "quota", "limit": "quota"},
                "durable-quota": {"kind": "quota", "limit": "durable-quota"},
            },
            "routers": {
                "anonymous": {"match": {"path": {"exact": "/anonymous"}}, "service": "local"},
                "api-key": {"match": {"path": {"exact": "/api-key"}}, "service": "local", "policies": ["api-key"]},
                "session": {"match": {"path": {"exact": "/session"}}, "service": "local", "policies": ["session"]},
                "oauth": {"match": {"path": {"exact": "/oauth"}}, "service": "local", "policies": ["oauth"]},
                "rate-limit": {"match": {"path": {"exact": "/rate-limit"}}, "service": "local", "policies": ["rate"]},
                "quota": {"match": {"path": {"exact": "/quota"}}, "service": "local", "policies": ["quota"]},
                "durable-quota": {"match": {"path": {"exact": "/durable-quota"}}, "service": "local", "policies": ["api-key", "durable-quota"]},
            },
        },
    }))
    .unwrap()
}

async fn start(gate: Arc<Gate>) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .fallback(crate::gate::service)
        .layer(Extension(gate));
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (url, task)
}

/// One request, returning its status and body latency in milliseconds.
async fn request(
    client: &Client<HttpConnector, Body>,
    url: &str,
    path: &str,
    credential: Option<&(&'static str, String)>,
) -> (u16, f64) {
    let mut request = Request::builder().uri(format!("{url}{path}"));
    if let Some((name, value)) = credential {
        request = request.header(*name, value);
    }
    let started = Instant::now();
    let Ok(response) = client.request(request.body(Body::empty()).unwrap()).await else {
        return (0, f64::NAN);
    };
    let status = response.status().as_u16();
    if response.into_body().collect().await.is_err() {
        return (0, f64::NAN);
    }
    (status, started.elapsed().as_secs_f64() * 1000.0)
}

fn seconds() -> u64 {
    std::env::var("STARGATE_BENCH_SECONDS").map_or(4, |value| value.parse().unwrap())
}

fn workers() -> usize {
    std::env::var("STARGATE_BENCH_WORKERS").map_or(16, |value| value.parse().unwrap())
}

fn selected(scenario: &Scenario) -> bool {
    match std::env::var("STARGATE_BENCH_SCENARIOS") {
        Ok(names) => names.split(',').any(|name| name == scenario.name),
        Err(_) => true,
    }
}

/// Round trips one request costs, counted as the backend reports them.
///
/// The in-process call keeps the counted commands in this task's scope, so it
/// measures the same per-request accounting the gateway exports; commands a
/// Lua script issues internally are not round trips and are excluded.
async fn round_trips(
    gate: &Arc<Gate>,
    scenario: &Scenario,
    credential: Option<&(&'static str, String)>,
) -> u32 {
    let mut builder = Request::builder().uri(scenario.path);
    if let Some((name, value)) = credential {
        builder = builder.header(*name, value);
    }
    let mut request = builder.body(Body::empty()).unwrap();
    request.extensions_mut().insert(gate.clone());
    request
        .extensions_mut()
        .insert(crate::srv::test_connection(SocketAddr::from((
            [127, 0, 0, 1],
            1000,
        ))));
    let (response, commands) = store::observe::count_commands(crate::gate::service(request)).await;
    assert_eq!(
        response.unwrap().status(),
        200,
        "{} round trips",
        scenario.name
    );
    commands
}

fn latencies(mut values: Vec<f64>) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    values.sort_by(f64::total_cmp);
    let at = |percent: usize| values[(values.len() * percent).div_ceil(100).saturating_sub(1)];
    json!({"p50":at(50), "p95":at(95), "p99":at(99), "max":values.last().unwrap()})
}

/// Server-side command counters, without this probe's own `INFO`.
async fn command_stats() -> BTreeMap<String, u64> {
    let mut connection: RedisConnection = use_store().get_connection();
    let info: String = redis::cmd("INFO")
        .arg("commandstats")
        .query_async(&mut connection)
        .await
        .unwrap();
    let mut stats = BTreeMap::new();
    for line in info.lines() {
        let Some(rest) = line.strip_prefix("cmdstat_") else {
            continue;
        };
        let Some((name, values)) = rest.split_once(':') else {
            continue;
        };
        let Some(calls) = values
            .split(',')
            .find_map(|field| field.strip_prefix("calls="))
        else {
            continue;
        };
        stats.insert(name.to_string(), calls.parse().unwrap());
    }
    stats
}

fn delta(before: &BTreeMap<String, u64>, after: &BTreeMap<String, u64>) -> BTreeMap<String, u64> {
    after
        .iter()
        .filter_map(|(name, calls)| {
            let issued = calls.saturating_sub(*before.get(name).unwrap_or(&0));
            (issued > 0 && name != "info").then_some((name.clone(), issued))
        })
        .collect()
}

/// Round trip of one application store read, like the backend table.
async fn redis_read_latency() -> Value {
    let key = store::keys::scoped(&format!("benchmark:{}", ulid::Ulid::generate()), "subject");
    let subject = crate::iam::auth::subject::Subject::new(
        key.clone(),
        crate::iam::auth::subject::SubjectType::User,
        Some(json!({"payload": "x".repeat(1024)})),
    );
    let store = use_store();
    store.set(&key, &subject, Some(60)).await.unwrap();
    let mut samples = Vec::new();
    for index in 0..1010 {
        let started = Instant::now();
        assert!(
            store
                .get::<crate::iam::auth::subject::Subject>(&key)
                .await
                .unwrap()
                .is_some()
        );
        if index >= 10 {
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }
    store.delete(&key).await.unwrap();
    latencies(samples)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "release benchmark requiring disposable PostgreSQL and Redis"]
async fn cluster_request_path_workload() {
    if cfg!(debug_assertions) {
        panic!("run this workload with --release");
    }
    assert_eq!(std::env::var("JWT_ALGORITHM").unwrap(), "HS256");
    session_jwt::init();
    crate::data::store::init().await.unwrap();
    crate::data::db::init().await.unwrap();
    let fixtures = Fixtures::create().await;
    let document = crate::data::db::active_or_seed(db::ent::ConfigKind::Gateway, || async {
        Ok(config().to_yaml_string()?)
    })
    .await
    .unwrap();
    let gate = Arc::new(
        Gate::from_document(
            Arc::new(use_store().clone()),
            crate::gate::snap::prepare_gateway_document(&document).unwrap(),
            PolicySnapshot::default(),
        )
        .await
        .unwrap(),
    );
    let (url, server) = start(gate.clone()).await;
    let client = Client::builder(TokioExecutor::new()).build(HttpConnector::new());
    let mut results = Vec::new();
    for scenario in SCENARIOS.iter().filter(|scenario| selected(scenario)) {
        let credential = fixtures.credential(scenario);
        // The warmup resolves the API key into its cached subject, so the
        // measured path is the steady-state Redis hit.
        let (status, _) = request(&client, &url, scenario.path, credential.as_ref()).await;
        assert_eq!(status, 200, "{} warmup", scenario.name);

        let round_trip_count = round_trips(&gate, scenario, credential.as_ref()).await;
        let before = command_stats().await;
        let started = Instant::now();
        let deadline = started + Duration::from_secs(seconds());
        let mut tasks = JoinSet::new();
        for _ in 0..workers() {
            let client = client.clone();
            let url = url.clone();
            let credential = fixtures.credential(scenario);
            let path = scenario.path;
            tasks.spawn(async move {
                let mut timings = Vec::new();
                let mut failed = 0_u64;
                while Instant::now() < deadline {
                    match request(&client, &url, path, credential.as_ref()).await {
                        (200, elapsed) => timings.push(elapsed),
                        (0, _) => failed += 1,
                        (status, _) => panic!("{path}: unexpected status {status}"),
                    }
                }
                (timings, failed)
            });
        }
        let (mut timings, mut failed) = (Vec::new(), 0_u64);
        while let Some(result) = tasks.join_next().await {
            let (samples, failures) = result.unwrap();
            timings.extend(samples);
            failed += failures;
        }
        let elapsed = started.elapsed().as_secs_f64();
        let commands = delta(&before, &command_stats().await);
        let issued: u64 = commands.values().sum();
        let requests = timings.len() as u64 + failed;
        assert!(requests > 0, "{}: no requests", scenario.name);
        let result = json!({
            "path": scenario.name,
            "seconds": elapsed,
            "workers": workers(),
            "requests": requests,
            "successful": timings.len(),
            "failed": failed,
            "requests_per_second": requests as f64 / elapsed,
            "successful_requests_per_second": timings.len() as f64 / elapsed,
            "body_latency_ms": latencies(timings),
            "redis_round_trips": round_trip_count,
            "redis_commands": issued,
            "redis_commands_per_request": issued as f64 / requests as f64,
            "redis_commands_by_name": commands,
        });
        println!("REQUEST_PATH_BENCH {result}");
        results.push(result);
    }
    assert!(!results.is_empty(), "no matching scenarios");
    server.abort();
    let output = json!({
        "workers": workers(),
        "seconds": seconds(),
        "redis_read_1k_ms": redis_read_latency().await,
        "paths": results,
    });
    println!("REQUEST_PATH_SUMMARY {output}");
    if let Ok(path) = std::env::var("STARGATE_BENCH_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&output).unwrap()).unwrap();
    }
}
