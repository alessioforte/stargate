//! Durable calendar quotas survive a state-store loss, and a SQL failure can
//! never admit traffic above the limit.
#![cfg(all(feature = "memory", feature = "sqlite"))]

use crate::data::db::with_test_database;
use crate::gate::Gate;
use crate::gate::authentication::TestAuthenticator;
use crate::gate::limits::SqlQuotaLedger;
use crate::iam::auth::identity::VerifiedIdentity;
use axum::body::Body;
use axum::extract::ConnectInfo;
use gate::cfg::Config;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use lim::QuotaLedger;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;

const IP: &str = "203.0.113.78:1000";

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
            "policies": {"quota": {"kind": "quota", "limit": "daily", "on_store_error": on_store_error}},
            "routers": {
                "quota": {
                    "match": {"path": {"exact": "/quota"}},
                    "service": "local",
                    "policies": ["quota"],
                    "quota_cost": 3
                }
            }
        }
    }))
    .unwrap()
}

fn gate(config: Config) -> Arc<Gate> {
    let prepared = crate::gate::prepare_config(config).unwrap();
    Arc::new(
        Gate::new(
            Arc::new(lim::State::new()),
            prepared,
            gate::PolicySnapshot::default(),
            Some(Arc::new(SqlQuotaLedger::new(crate::data::db::service()))),
        )
        .unwrap(),
    )
}

fn request(gate: &Arc<Gate>) -> Request<Body> {
    let mut request = Request::builder()
        .uri("/quota")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(gate.clone());
    request.extensions_mut().insert(TestAuthenticator {
        verify: Arc::new(|_| VerifiedIdentity::anonymous()),
    });
    request
        .extensions_mut()
        .insert(ConnectInfo(IP.parse::<SocketAddr>().unwrap()));
    request
}

async fn send(gate: &Arc<Gate>) -> axum::response::Response {
    crate::gate::service(request(gate)).await.unwrap()
}

async fn code(response: axum::response::Response) -> String {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice::<Value>(&body).unwrap()["code"]
        .as_str()
        .unwrap()
        .into()
}

#[tokio::test]
async fn a_flushed_state_store_does_not_reset_the_calendar_quota() {
    with_test_database(async {
        let first = gate(config("deny"));
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
        let restarted = gate(config("deny"));
        let denied = send(&restarted).await;
        assert_eq!(denied.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(denied.headers()["x-quota-remaining"], "0");
        assert_eq!(denied.headers()["x-quota-limit"], "6");
    })
    .await;
}

#[tokio::test]
async fn a_durable_quota_is_fail_closed_even_when_the_policy_allows_fail_open() {
    with_test_database(async {
        struct Unavailable;
        #[async_trait::async_trait]
        impl QuotaLedger for Unavailable {
            async fn reserve(
                &self,
                _bucket: &str,
                _definition: &str,
                _window_start: i64,
                _window_end: i64,
                _limit: u64,
                _units: u64,
            ) -> lim::Result<Option<u64>> {
                Err(lim::RateLimitError::BackendError("unavailable".into()))
            }
        }

        let prepared = crate::gate::prepare_config(config("allow")).unwrap();
        let gate = Arc::new(
            Gate::new(
                Arc::new(lim::State::new()),
                prepared,
                gate::PolicySnapshot::default(),
                Some(Arc::new(Unavailable)),
            )
            .unwrap(),
        );
        let response = send(&gate).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(code(response).await, "gateway.limiter_unavailable");
    })
    .await;
}
