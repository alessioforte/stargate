use super::authentication::TestAuthenticator;
use crate::etc::{
    gate::{Gate, prepare_config, test_support},
    guard::VerifiedIdentity,
    sub::{Subject, SubjectType},
};
use axum::{body::Body, extract::ConnectInfo, response::Response};
use gate::cfg::Config;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;

fn config(burst: u32) -> Config {
    serde_json::from_value(json!({
        "schema": gate::cfg::SCHEMA,
        "ingress": {"limit":"ingress", "timeout":"50ms"},
        "runtime": {"primary_concurrency":1, "request_timeout":"2s"},
        "limits": {
            "ingress": {"strategy":"gcra", "params":{"max_burst":burst, "replenish_1_per":"1h"}},
            "default": {"strategy":"gcra", "params":{"max_burst":100, "replenish_1_per":"1h"}}
        },
        "http": {
            "services": {"local":{"kind":"direct_response", "status":200, "body":{"text":"ready"}}},
            "policies": {
                "auth":{"kind":"auth", "strategies":["jwt", "api_key"]},
                "acl":{"kind":"access_control", "resource":"private", "env":"none"}
            },
            "routers": {
                "open":{"match":{"path":{"exact":"/open"}}, "service":"local"},
                "protected":{"match":{"path":{"exact":"/protected"}}, "service":"local", "policies":["auth"]},
                "denied":{"match":{"path":{"exact":"/denied"}}, "service":"local", "policies":["acl"]}
            }
        }
    })).unwrap()
}

#[derive(Default)]
struct AuthenticationCalls {
    verification: AtomicUsize,
    lookups: AtomicUsize,
}

impl AuthenticationCalls {
    fn boundary(self: &Arc<Self>, identity: VerifiedIdentity) -> TestAuthenticator {
        let calls = self.clone();
        TestAuthenticator {
            verify: Arc::new(move |request| {
                calls.verification.fetch_add(1, Ordering::SeqCst);
                // Model the backing lookup inside the verification boundary. A
                // denied admission must never enter either stage of authentication.
                if request.headers().contains_key("x-api-key")
                    || request.headers().contains_key("authorization")
                {
                    calls.lookups.fetch_add(1, Ordering::SeqCst);
                }
                identity.clone()
            }),
        }
    }

    fn counts(&self) -> (usize, usize) {
        (
            self.verification.load(Ordering::SeqCst),
            self.lookups.load(Ordering::SeqCst),
        )
    }
}

fn request(gate: &Arc<Gate>, auth: &TestAuthenticator, path: &str, peer: &str) -> Request<Body> {
    let mut request = Request::builder().uri(path).body(Body::empty()).unwrap();
    request.extensions_mut().insert(gate.clone());
    request.extensions_mut().insert(auth.clone());
    request
        .extensions_mut()
        .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    request
}

async fn send(request: Request<Body>) -> Response {
    super::service(request).await.unwrap()
}

async fn error_code(response: Response) -> String {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice::<Value>(&body).unwrap()["code"]
        .as_str()
        .unwrap()
        .into()
}

#[tokio::test]
async fn invalid_credentials_protected_and_missing_routes_consume_ingress_before_authentication() {
    for (path, header, credential, status) in [
        ("/open", "x-api-key", "invalid-key", StatusCode::OK),
        (
            "/open",
            "authorization",
            "Bearer invalid-token",
            StatusCode::OK,
        ),
        (
            "/protected",
            "authorization",
            "Bearer invalid-token",
            StatusCode::UNAUTHORIZED,
        ),
        (
            "/missing",
            "x-api-key",
            "invalid-key",
            StatusCode::NOT_FOUND,
        ),
    ] {
        let gate = Arc::new(test_support::gate(config(2)));
        let calls = Arc::new(AuthenticationCalls::default());
        let auth = calls.boundary(VerifiedIdentity::anonymous());
        for _ in 0..2 {
            let mut req = request(&gate, &auth, path, "203.0.113.1:1000");
            req.headers_mut()
                .insert(header, credential.parse().unwrap());
            assert_eq!(send(req).await.status(), status);
        }
        assert_eq!(calls.counts(), (2, 2));
        let mut denied = request(&gate, &auth, path, "203.0.113.1:1000");
        denied
            .headers_mut()
            .insert(header, "new-invalid-credential".parse().unwrap());
        let response = send(denied).await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()["x-ratelimit-scope"], "ingress");
        assert!(
            response.headers()["retry-after"]
                .to_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                > 0
        );
        assert_eq!(
            error_code(response).await,
            "gateway.ingress_rate_limit_exceeded"
        );
        assert_eq!(calls.counts(), (2, 2));
    }
}

#[tokio::test]
async fn acl_denials_and_authenticated_identities_share_the_same_ip_admission() {
    let gate = Arc::new(test_support::gate(config(1)));
    let calls = Arc::new(AuthenticationCalls::default());
    let identity = |id: &str| {
        VerifiedIdentity::test_jwt(
            Subject::new(id.into(), SubjectType::User, None),
            "sid".into(),
            None,
        )
    };
    let auth = calls.boundary(identity("first"));
    let response = send(request(&gate, &auth, "/denied", "203.0.113.2:1000")).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let other_identity = calls.boundary(identity("second"));
    assert_eq!(
        send(request(&gate, &other_identity, "/open", "203.0.113.2:2000"))
            .await
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(calls.counts(), (1, 0));
}

#[tokio::test]
async fn untrusted_forwarding_headers_cannot_change_the_ingress_bucket() {
    let gate = Arc::new(test_support::gate(config(1)));
    let calls = Arc::new(AuthenticationCalls::default());
    let auth = calls.boundary(VerifiedIdentity::anonymous());
    assert_eq!(
        send(request(&gate, &auth, "/open", "203.0.113.3:1000"))
            .await
            .status(),
        StatusCode::OK
    );
    let mut forged = request(&gate, &auth, "/open", "203.0.113.3:2000");
    for (name, value) in [
        ("x-forwarded-for", "198.51.100.99"),
        ("x-real-ip", "198.51.100.98"),
        ("forwarded", "for=198.51.100.97"),
    ] {
        forged.headers_mut().insert(name, value.parse().unwrap());
    }
    assert_eq!(send(forged).await.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(calls.counts(), (1, 0));
    assert_eq!(
        send(request(&gate, &auth, "/open", "203.0.113.4:1000"))
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn the_same_named_ingress_and_resource_limit_use_distinct_buckets() {
    let mut config = config(2);
    config.http.policies.insert(
        "resource".into(),
        serde_json::from_value(json!({"kind":"rate_limit", "limit":"ingress"})).unwrap(),
    );
    config
        .http
        .routers
        .get_mut("open")
        .unwrap()
        .policies
        .push("resource".into());
    let gate = Arc::new(test_support::gate(config));
    let calls = Arc::new(AuthenticationCalls::default());
    let auth = calls.boundary(VerifiedIdentity::anonymous());
    for remaining in [1, 0] {
        let response = send(request(&gate, &auth, "/open", "203.0.113.5:1000")).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["x-ratelimit-remaining"],
            remaining.to_string()
        );
        assert_eq!(response.headers()["x-ratelimit-scope"], "subject");
        drop(response);
    }
    assert_eq!(
        send(request(&gate, &auth, "/open", "203.0.113.5:1000"))
            .await
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(calls.counts(), (2, 0));
}

#[tokio::test]
async fn authenticated_quota_costs_and_org_ip_fallback_remain_independent() {
    let mut value = serde_json::to_value(config(10)).unwrap();
    value["limits"]["quota"] =
        json!({"strategy":"quota_tracker", "params":{"limit":6,"period":"hour"}});
    value["http"]["policies"]["quota"] = json!({"kind":"quota", "limit":"quota"});
    value["http"]["policies"]["org-rate"] =
        json!({"kind":"rate_limit", "limit":"ingress", "scope":"org", "on_missing":"ip_fallback"});
    value["http"]["policies"]["org-quota"] =
        json!({"kind":"quota", "limit":"quota", "scope":"org", "on_missing":"ip_fallback"});
    value["http"]["routers"]["open"]["policies"] = json!(["quota", "org-rate", "org-quota"]);
    value["http"]["routers"]["open"]["quota_cost"] = json!(3);
    let gate = Arc::new(test_support::gate(serde_json::from_value(value).unwrap()));
    let calls = Arc::new(AuthenticationCalls::default());
    let auth = calls.boundary(VerifiedIdentity::test_api_key(Subject::new(
        "key".into(),
        SubjectType::ApiKey,
        None,
    )));
    for remaining in [3, 0] {
        let response = send(request(&gate, &auth, "/open", "203.0.113.6:1000")).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["x-quota-remaining"],
            remaining.to_string()
        );
        drop(response);
    }
    let response = send(request(&gate, &auth, "/open", "203.0.113.6:1000")).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error_code(response).await, "gateway.quota_exceeded");
    assert_eq!(calls.counts(), (3, 0));
}

#[tokio::test]
async fn process_overload_precedes_ingress_and_failed_reload_preserves_admission() {
    let gate = Arc::new(test_support::gate(config(1)));
    let calls = Arc::new(AuthenticationCalls::default());
    let auth = calls.boundary(VerifiedIdentity::anonymous());
    let occupied = gate.resources.admit_primary().unwrap();
    let response = send(request(&gate, &auth, "/open", "203.0.113.7:1000")).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(error_code(response).await, "gateway.overloaded");
    assert_eq!(calls.counts(), (0, 0));
    drop(occupied);
    let previous = gate.snapshot();
    for (limit, timeout) in [("missing", "50ms"), ("ingress", "0s")] {
        let mut invalid = config(1);
        invalid.ingress.limit = limit.into();
        invalid.ingress.timeout = timeout.into();
        assert!(prepare_config(invalid).is_err());
        assert!(Arc::ptr_eq(&gate.snapshot(), &previous));
    }
    assert_eq!(
        send(request(&gate, &auth, "/open", "203.0.113.7:1000"))
            .await
            .status(),
        StatusCode::OK
    );
}

struct AdmissionBackend {
    entered: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
    stall: bool,
}

struct PendingCheck(Arc<AtomicBool>);
impl Drop for PendingCheck {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl lim::RateLimit for AdmissionBackend {
    fn kind(&self) -> lim::LimitKind {
        lim::LimitKind::Rate
    }

    async fn check(&self, key: &str, cost: u64) -> lim::Result<lim::RateLimitDecision> {
        assert_eq!(key, "ingress:ingress:ip:203.0.113.8");
        assert_eq!(cost, 1);
        let _pending = PendingCheck(self.dropped.clone());
        self.entered.store(true, Ordering::SeqCst);
        if self.stall {
            std::future::pending::<()>().await;
        }
        Err(lim::RateLimitError::BackendError(
            "admission backend unavailable".into(),
        ))
    }
}

#[tokio::test]
async fn ingress_backend_failure_and_timeout_fail_closed_and_release_process_admission() {
    for stall in [false, true] {
        let entered = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let mut limiter = lim::Limiter::new();
        limiter.add_limit(
            "ingress".into(),
            Box::new(AdmissionBackend {
                entered: entered.clone(),
                dropped: dropped.clone(),
                stall,
            }),
        );
        let gate = Arc::new(test_support::gate_with_limiter(config(1), limiter));
        let calls = Arc::new(AuthenticationCalls::default());
        let auth = calls.boundary(VerifiedIdentity::anonymous());
        let response = tokio::time::timeout(
            Duration::from_secs(1),
            send(request(&gate, &auth, "/open", "203.0.113.8:1000")),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(error_code(response).await, "gateway.ingress_unavailable");
        assert!(entered.load(Ordering::SeqCst));
        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(calls.counts(), (0, 0));
        assert_eq!(gate.resources.available().0, 1);
    }
    let gate = Arc::new(test_support::gate_with_limiter(
        config(1),
        lim::Limiter::new(),
    ));
    let calls = Arc::new(AuthenticationCalls::default());
    let auth = calls.boundary(VerifiedIdentity::anonymous());
    assert_eq!(
        send(request(&gate, &auth, "/open", "203.0.113.8:1000"))
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(calls.counts(), (0, 0));
}

#[tokio::test]
async fn total_deadline_shutdown_and_client_drop_cancel_pending_ingress_checks() {
    for stop in ["total", "shutdown", "client_drop"] {
        let entered = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let mut limiter = lim::Limiter::new();
        limiter.add_limit(
            "ingress".into(),
            Box::new(AdmissionBackend {
                entered: entered.clone(),
                dropped: dropped.clone(),
                stall: true,
            }),
        );
        let mut config = config(1);
        config.ingress.timeout = "1s".into();
        if stop == "total" {
            config.runtime.request_timeout = "20ms".into();
        }
        let gate = Arc::new(test_support::gate_with_limiter(config, limiter));
        let calls = Arc::new(AuthenticationCalls::default());
        let auth = calls.boundary(VerifiedIdentity::anonymous());
        let pending = tokio::spawn(send(request(&gate, &auth, "/open", "203.0.113.8:1000")));
        tokio::time::timeout(Duration::from_secs(1), async {
            while !entered.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        match stop {
            "shutdown" => gate.resources.begin_shutdown(),
            "client_drop" => pending.abort(),
            _ => {}
        }
        let result = tokio::time::timeout(Duration::from_secs(1), pending)
            .await
            .unwrap();
        if stop == "client_drop" {
            assert!(result.unwrap_err().is_cancelled());
        } else {
            let response = result.unwrap();
            assert_eq!(
                response.status(),
                if stop == "total" {
                    StatusCode::GATEWAY_TIMEOUT
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                }
            );
            assert_eq!(
                error_code(response).await,
                if stop == "total" {
                    "gateway.timeout"
                } else {
                    "gateway.cancelled"
                }
            );
        }
        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(gate.resources.available().0, 1);
        assert_eq!(calls.counts(), (0, 0));
    }
}

#[tokio::test]
async fn requests_pin_the_ingress_binding_and_reload_preserves_existing_allowances() {
    let original = config(2);
    let gate = Arc::new(test_support::gate(original.clone()));
    let calls = Arc::new(AuthenticationCalls::default());
    let auth = calls.boundary(VerifiedIdentity::anonymous());
    assert_eq!(
        send(request(&gate, &auth, "/open", "203.0.113.10:1000"))
            .await
            .status(),
        StatusCode::OK
    );
    let pause = super::IngressPause {
        pinned: Arc::new(tokio::sync::Barrier::new(2)),
        resume: Arc::new(tokio::sync::Barrier::new(2)),
    };
    let mut req = request(&gate, &auth, "/open", "203.0.113.10:1000");
    req.extensions_mut().insert(pause.clone());
    let pending = tokio::spawn(send(req));
    pause.pinned.wait().await;
    let mut changed = config(1);
    let spec = changed.limits.swap_remove("ingress").unwrap();
    changed.limits.insert("new-ingress".into(), spec);
    changed.ingress.limit = "new-ingress".into();
    gate.activate(prepare_config(changed).unwrap()).unwrap();
    pause.resume.wait().await;
    assert_eq!(pending.await.unwrap().status(), StatusCode::OK);
    assert_eq!(
        send(request(&gate, &auth, "/open", "203.0.113.10:1000"))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(request(&gate, &auth, "/open", "203.0.113.10:1000"))
            .await
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    gate.activate(prepare_config(original).unwrap()).unwrap();
    assert_eq!(
        send(request(&gate, &auth, "/open", "203.0.113.10:1000"))
            .await
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(calls.counts(), (3, 0));
}

#[tokio::test]
async fn probes_are_exempt_and_iam_and_admin_keep_their_existing_middleware() {
    let mut config = config(1);
    // The gateway's implicit resource check and IAM middleware retain their
    // existing shared default/IP bucket, independently of ingress admission.
    config.limits.insert(
        "default".into(),
        serde_json::from_value(
            json!({"strategy":"gcra", "params":{"max_burst":3, "replenish_1_per":"1h"}}),
        )
        .unwrap(),
    );
    let gate = Arc::new(test_support::gate(config));
    let calls = Arc::new(AuthenticationCalls::default());
    let auth = calls.boundary(VerifiedIdentity::anonymous());
    let app = crate::api::server_router(crate::etc::health::Lifecycle::default());
    assert_eq!(
        app.clone()
            .oneshot(request(&gate, &auth, "/open", "203.0.113.9:1000"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.clone()
            .oneshot(request(&gate, &auth, "/open", "203.0.113.9:1000"))
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    let occupied = gate.resources.admit_primary().unwrap();
    for path in ["/livez", "/readyz", "/health"] {
        for method in [http::Method::GET, http::Method::HEAD] {
            let mut req = request(&gate, &auth, path, "203.0.113.9:1000");
            *req.method_mut() = method;
            let response = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                response.status(),
                if path == "/livez" {
                    StatusCode::OK
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                }
            );
            assert!(!response.headers().contains_key("x-ratelimit-limit"));
        }
    }
    drop(occupied);
    let response = app
        .clone()
        .oneshot(request(&gate, &auth, "/docs/errors", "203.0.113.9:1000"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-ratelimit-remaining"], "1");
    let response = app
        .clone()
        .oneshot(request(
            &gate,
            &auth,
            "/admin/configurations",
            "203.0.113.9:1000",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(response.headers()["x-ratelimit-remaining"], "0");
    let response = app
        .oneshot(request(&gate, &auth, "/docs/errors", "203.0.113.9:1000"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error_code(response).await, "gateway.rate_limit_exceeded");
    assert_eq!(calls.counts(), (1, 0));
}
