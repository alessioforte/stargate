//! Durable calendar quotas survive a state-store loss, belong to an
//! authenticated subject, and a SQL failure can never admit traffic above the
//! limit.
#![cfg(all(feature = "memory", feature = "sqlite"))]

use crate::data::db::with_test_database;
use crate::gate::Gate;
use crate::gate::authentication::TestAuthenticator;
use crate::gate::limits::SqlQuotaLedger;
use crate::gate::snap::prepare_gateway_document;
use crate::iam::auth::identity::VerifiedIdentity;
use crate::iam::auth::subject::{Subject, SubjectType};
use axum::body::Body;
use db::ent::{ConfigDocument, ConfigKind, ConfigSaveOutcome, ConfigSource, TrustedAuditContext};
use gate::cfg::Config;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use lim::{QuotaBalance, QuotaLedger, Reservation, ReservationRequest};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
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

fn sql_ledger() -> Arc<dyn QuotaLedger> {
    Arc::new(SqlQuotaLedger::new(crate::data::db::service()))
}

fn stored_gate(document: &ConfigDocument) -> Arc<Gate> {
    Arc::new(
        Gate::new(
            Arc::new(lim::State::new()),
            prepare_gateway_document(document).unwrap(),
            gate::PolicySnapshot::default(),
            Some(sql_ledger()),
        )
        .unwrap(),
    )
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
    let ConfigSaveOutcome::Saved(document) = outcome else {
        panic!("expected a saved quota configuration, got {outcome:?}");
    };
    document
}

fn request(gate: &Arc<Gate>, identity: fn() -> VerifiedIdentity) -> Request<Body> {
    let mut request = Request::builder()
        .uri("/quota")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(gate.clone());
    request.extensions_mut().insert(TestAuthenticator {
        verify: Arc::new(move |_| identity()),
    });
    request.extensions_mut().insert(crate::srv::test_connection(
        IP.parse::<SocketAddr>().unwrap(),
    ));
    request
}

fn api_key() -> VerifiedIdentity {
    VerifiedIdentity::test_api_key(Subject::new(
        API_KEY_ID.to_string(),
        SubjectType::ApiKey,
        None,
    ))
}

async fn send(gate: &Arc<Gate>) -> axum::response::Response {
    crate::gate::service(request(gate, api_key)).await.unwrap()
}

async fn code(response: axum::response::Response) -> String {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice::<Value>(&body).unwrap()["code"]
        .as_str()
        .unwrap()
        .into()
}

/// Counts reservations and grants them all.
#[derive(Default)]
struct CountingLedger(AtomicUsize);

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

#[tokio::test]
async fn a_flushed_state_store_does_not_reset_the_calendar_quota() {
    with_test_database(async {
        let first = gate(config("deny"), sql_ledger());
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
        let restarted = gate(config("deny"), sql_ledger());
        let denied = send(&restarted).await;
        assert_eq!(denied.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(denied.headers()["x-quota-remaining"], "0");
        assert_eq!(denied.headers()["x-quota-limit"], "6");
    })
    .await;
}

#[tokio::test]
async fn publishing_a_calendar_change_preserves_spend_and_fences_the_old_gateway() {
    with_test_database(async {
        let first = save_quota(10, 5, "day", None).await;
        let old = stored_gate(&first);
        assert_eq!(send(&old).await.status(), StatusCode::OK);

        let second = save_quota(10, 5, "month", Some(first)).await;
        let current = stored_gate(&second);
        let stale = send(&old).await;
        assert_eq!(stale.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(code(stale).await, "gateway.limiter_unavailable");

        let last_allowed = send(&current).await;
        assert_eq!(last_allowed.status(), StatusCode::OK);
        assert_eq!(last_allowed.headers()["x-quota-remaining"], "0");
        assert_eq!(send(&current).await.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            send(&stored_gate(&second)).await.status(),
            StatusCode::TOO_MANY_REQUESTS
        );
    })
    .await;
}

#[tokio::test]
async fn a_fresh_gateway_reports_the_maximum_balance_as_quota_exhaustion() {
    with_test_database(async {
        const LIMIT: u64 = i64::MAX as u64;
        let first = save_quota(LIMIT, LIMIT, "day", None).await;
        assert_eq!(send(&stored_gate(&first)).await.status(), StatusCode::OK);

        let second = save_quota(LIMIT, 1, "day", Some(first)).await;
        let fresh = stored_gate(&second);
        let denied = send(&fresh).await;
        assert_eq!(denied.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(denied.headers()["x-quota-remaining"], "0");
        assert_eq!(denied.headers()["x-quota-limit"], LIMIT.to_string());
        assert_eq!(send(&fresh).await.status(), StatusCode::TOO_MANY_REQUESTS);
    })
    .await;
}

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
