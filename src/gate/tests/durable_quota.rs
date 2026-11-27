//! Durable calendar quotas survive a state-store loss, belong to an
//! authenticated subject, and a SQL failure can never admit traffic above the
//! limit.
#[cfg(feature = "sqlite")]
use crate::data::db::with_test_database;
use crate::gate::Gate;
use crate::gate::authentication::TestAuthenticator;
use crate::gate::snap::prepare_gateway_document;
use crate::iam::auth::identity::VerifiedIdentity;
use crate::iam::auth::subject::{Subject, SubjectType};
use axum::body::Body;
use db::ent::{ConfigDocument, ConfigKind, ConfigSaveOutcome, ConfigSource, TrustedAuditContext};
use gate::cfg::Config;
use http::{Request, StatusCode};
#[cfg(feature = "sqlite")]
use http_body_util::BodyExt;
#[cfg(feature = "sqlite")]
use lim::{QuotaBalance, QuotaLedger, Reservation, ReservationRequest};
#[cfg(feature = "sqlite")]
use serde_json::Value;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
#[cfg(feature = "sqlite")]
use std::sync::atomic::{AtomicUsize, Ordering};

const IP: &str = "203.0.113.78:1000";
const API_KEY_ID: &str = "01JZ0000000000000000000KEY";

fn config(on_store_error: &str) -> Config {
    serde_json::from_value(json!({
        "schema": "stargate/v1",
        "ingress": {"limit": "ingress", "timeout": "250ms"},
        "limits": {
            "ingress": {"strategy": "gcra", "params": {"max_burst": 1000, "replenish_1_per": "1us"}},
            "default": {"strategy": "gcra", "params": {"max_burst": 1000, "replenish_1_per": "1us"}},
            "daily": {"strategy": "quota_tracker", "params": {"limit": 6, "period": "day", "timezone": "UTC"}}
        },
        "http": {
            "services": {"local": {"kind": "direct_response", "status": 200}},
            "policies": {
                "keys": {"kind": "auth", "strategies": ["api_key"]},
                "quota": {"kind": "quota", "limit": "daily", "on_store_error": on_store_error}
            },
            "routers": {
                "quota": {
                    "match": {"path": {"exact": "/quota"}},
                    "service": "local",
                    "policies": ["keys", "quota"],
                    "quota_cost": 3
                }
            }
        }
    }))
    .unwrap()
}

#[cfg(feature = "sqlite")]
fn gate(config: Config, ledger: Arc<dyn QuotaLedger>) -> Arc<Gate> {
    let prepared = crate::gate::prepare_config(config).unwrap();
    Arc::new(
        Gate::new(
            Arc::new(lim::State::new()),
            prepared,
            gate::PolicySnapshot::default(),
            Some(ledger),
        )
        .unwrap(),
    )
}

async fn open_gateway(document: &ConfigDocument, store: Arc<lim::State>) -> Arc<Gate> {
    Arc::new(
        Gate::from_document(
            store,
            prepare_gateway_document(document).unwrap(),
            gate::PolicySnapshot::default(),
        )
        .await
        .unwrap(),
    )
}

#[cfg(feature = "sqlite")]
async fn stored_gate(document: &ConfigDocument) -> Arc<Gate> {
    open_gateway(document, Arc::new(lim::State::new())).await
}

async fn save_quota(
    limit: u64,
    cost: u64,
    period: &str,
    previous: Option<ConfigDocument>,
) -> ConfigDocument {
    let mut raw = serde_json::to_value(config("allow")).unwrap();
    raw["limits"]["daily"]["params"]["limit"] = json!(limit);
    raw["limits"]["daily"]["params"]["period"] = json!(period);
    raw["http"]["routers"]["quota"]["quota_cost"] = json!(cost);
    let config: Config = serde_json::from_value(raw).unwrap();
    let outcome = crate::data::db::save_config_document(
        ConfigKind::Gateway,
        config.to_yaml_string().unwrap(),
        previous.map(|document| document.revision),
        ConfigSource::CliImport,
        TrustedAuditContext::system_control_plane(),
    )
    .await
    .unwrap();
    match outcome {
        ConfigSaveOutcome::Saved(document) | ConfigSaveOutcome::Unchanged(document) => document,
        other => panic!("expected a saved quota configuration, got {other:?}"),
    }
}

async fn verify_capacity_change(store: Arc<lim::State>) {
    use crate::data::db;
    use crate::gate::{ConfigActivation, save_gateway_config};
    use ::db::ent::{TrustedAdminActor, TrustedAuditRequest};

    let previous = db::get_active_config_document(ConfigKind::Gateway)
        .await
        .unwrap();
    let original = save_quota(6, 3, "day", previous).await;
    let gate = open_gateway(&original, store).await;
    let snapshot = gate.snapshot();
    let id = ulid::Ulid::generate().to_string();
    let issue = || {
        crate::gate::service(request(&gate, {
            let id = id.clone();
            move || {
                VerifiedIdentity::test_api_key(Subject::new(id.clone(), SubjectType::ApiKey, None))
            }
        }))
    };
    let first = issue().await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(first.headers()["x-quota-remaining"], "3");

    let mut config = serde_json::to_value(config("allow")).unwrap();
    config["runtime"]["primary_concurrency"] = json!(2048);
    let context = TrustedAuditContext::admin_control_plane(
        TrustedAdminActor::admin_key(API_KEY_ID),
        TrustedAuditRequest::from_http("quota-capacity-change", None, None, None),
    );
    let (saved, activation) = save_gateway_config(
        gate.clone(),
        serde_json::from_value(config).unwrap(),
        original.revision,
        context,
    )
    .await
    .unwrap();
    assert_eq!(saved.version, original.version + 1);
    assert!(matches!(activation, ConfigActivation::RestartRequired));
    assert!(Arc::ptr_eq(&snapshot, &gate.snapshot()));
    // This cost requires a new SQL batch of three units.
    let next = issue().await.unwrap();
    assert_eq!(next.status(), StatusCode::OK);
    assert_eq!(next.headers()["x-quota-remaining"], "0");
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn capacity_saved_for_restart_keeps_durable_quota_available() {
    with_test_database(verify_capacity_change(Arc::new(lim::State::new()))).await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "requires disposable PostgreSQL and Redis services"]
async fn postgres_capacity_saved_for_restart_keeps_durable_quota_available() {
    let url = std::env::var("TEST_DATABASE_URL").expect("disposable PostgreSQL URL");
    let database = db::service::init(&url).await.unwrap();
    database.migrate().await.unwrap();
    let url = std::env::var("TEST_REDIS_URL").expect("disposable Redis URL");
    let store = Arc::new(lim::State::new(&url).await.unwrap());
    crate::data::db::with_test_service(database, verify_capacity_change(store)).await;
}

fn request(
    gate: &Arc<Gate>,
    identity: impl Fn() -> VerifiedIdentity + Send + Sync + 'static,
) -> Request<Body> {
    let mut request = Request::builder()
        .uri("/quota")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(gate.clone());
    request.extensions_mut().insert(TestAuthenticator {
        verify: Arc::new(move |_, _| identity()),
    });
    request.extensions_mut().insert(crate::srv::test_connection(
        IP.parse::<SocketAddr>().unwrap(),
    ));
    request
}

#[cfg(feature = "sqlite")]
fn api_key() -> VerifiedIdentity {
    VerifiedIdentity::test_api_key(Subject::new(
        API_KEY_ID.to_string(),
        SubjectType::ApiKey,
        None,
    ))
}

#[cfg(feature = "sqlite")]
async fn send(gate: &Arc<Gate>) -> axum::response::Response {
    crate::gate::service(request(gate, api_key)).await.unwrap()
}

#[cfg(feature = "sqlite")]
async fn code(response: axum::response::Response) -> String {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice::<Value>(&body).unwrap()["code"]
        .as_str()
        .unwrap()
        .into()
}

/// Counts reservations and grants them all.
#[cfg(feature = "sqlite")]
#[derive(Default)]
struct CountingLedger(AtomicUsize);

#[cfg(feature = "sqlite")]
#[async_trait::async_trait]
impl QuotaLedger for CountingLedger {
    async fn reserve(&self, request: ReservationRequest<'_>) -> lim::Result<Reservation> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Reservation::Granted(QuotaBalance {
            used: request.units,
            window: request.window,
        }))
    }
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn a_flushed_state_store_does_not_reset_the_calendar_quota() {
    with_test_database(async {
        let document = save_quota(6, 3, "day", None).await;
        let first = stored_gate(&document).await;
        for remaining in [3, 0] {
            let response = send(&first).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()["x-quota-remaining"],
                remaining.to_string()
            );
        }
        assert_eq!(send(&first).await.status(), StatusCode::TOO_MANY_REQUESTS);

        // A new process with an empty state store: the balance is still in SQL.
        let restarted = stored_gate(&document).await;
        let denied = send(&restarted).await;
        assert_eq!(denied.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(denied.headers()["x-quota-remaining"], "0");
        assert_eq!(denied.headers()["x-quota-limit"], "6");
    })
    .await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn publishing_a_calendar_change_preserves_spend_and_fences_the_old_gateway() {
    with_test_database(async {
        let first = save_quota(10, 5, "day", None).await;
        let old = stored_gate(&first).await;
        assert_eq!(send(&old).await.status(), StatusCode::OK);

        let second = save_quota(10, 5, "month", Some(first)).await;
        let current = stored_gate(&second).await;
        let stale = send(&old).await;
        assert_eq!(stale.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(code(stale).await, "gateway.limiter_unavailable");

        let last_allowed = send(&current).await;
        assert_eq!(last_allowed.status(), StatusCode::OK);
        assert_eq!(last_allowed.headers()["x-quota-remaining"], "0");
        assert_eq!(send(&current).await.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            send(&stored_gate(&second).await).await.status(),
            StatusCode::TOO_MANY_REQUESTS
        );
    })
    .await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn a_fresh_gateway_reports_the_maximum_balance_as_quota_exhaustion() {
    with_test_database(async {
        const LIMIT: u64 = i64::MAX as u64;
        let first = save_quota(LIMIT, LIMIT, "day", None).await;
        assert_eq!(
            send(&stored_gate(&first).await).await.status(),
            StatusCode::OK
        );

        let second = save_quota(LIMIT, 1, "day", Some(first)).await;
        let fresh = stored_gate(&second).await;
        let denied = send(&fresh).await;
        assert_eq!(denied.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(denied.headers()["x-quota-remaining"], "0");
        assert_eq!(denied.headers()["x-quota-limit"], LIMIT.to_string());
        assert_eq!(send(&fresh).await.status(), StatusCode::TOO_MANY_REQUESTS);
    })
    .await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn a_durable_quota_is_fail_closed_even_when_the_policy_allows_fail_open() {
    with_test_database(async {
        struct Unavailable;
        #[async_trait::async_trait]
        impl QuotaLedger for Unavailable {
            async fn reserve(&self, _: ReservationRequest<'_>) -> lim::Result<Reservation> {
                Err(lim::RateLimitError::BackendError("unavailable".into()))
            }
        }

        let gate = gate(config("allow"), Arc::new(Unavailable));
        let response = send(&gate).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(code(response).await, "gateway.limiter_unavailable");
    })
    .await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn an_anonymous_request_never_charges_a_durable_quota() {
    // Compilation rejects a calendar quota on a route without authentication,
    // so build the route with a short quota and swap a durable tracker in.
    let mut config = config("deny");
    config.http.routers.get_mut("quota").unwrap().policies = vec!["quota".into()];
    let mut short = serde_json::to_value(&config).unwrap();
    short["limits"]["daily"] =
        json!({"strategy": "quota_tracker", "params": {"limit": 6, "period": "hour"}});
    let short: Config = serde_json::from_value(short).unwrap();

    let ledger = Arc::new(CountingLedger::default());
    let durable = lim::DurableQuotas::new(ledger.clone());
    let mut limiter = lim::Limiter::new();
    for limit in crate::gate::prepare_config(short.clone())
        .unwrap()
        .config
        .limits()
    {
        let built = if limit.name == "daily" {
            Box::new(
                lim::strategies::quota_tracker::DurableQuotaTracker::new(
                    &durable,
                    "daily",
                    lim::CachedClock::new(),
                    6,
                    lim::strategies::quota_tracker::CalendarWindow::parse(
                        lim::strategies::quota_tracker::CalendarPeriod::Day,
                        "UTC",
                        None,
                    )
                    .unwrap(),
                )
                .unwrap(),
            ) as Box<dyn lim::RateLimit>
        } else {
            limit
                .clone()
                .build(Arc::new(lim::State::new()), lim::CachedClock::new(), None)
                .unwrap()
        };
        limiter.add_limit(limit.name.clone(), built);
    }
    let gate = Arc::new(crate::gate::test_support::gate_with_limiter(short, limiter));

    let response = crate::gate::service(request(&gate, VerifiedIdentity::anonymous))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(code(response).await, "gateway.limit_configuration_invalid");
    assert_eq!(ledger.0.load(Ordering::SeqCst), 0);

    // The same route charges an authenticated subject.
    let response = crate::gate::service(request(&gate, api_key)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(ledger.0.load(Ordering::SeqCst), 1);
}
