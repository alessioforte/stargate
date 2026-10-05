use super::authentication::TestAuthenticator;
use crate::etc::{
    gate::{Gate, test_support},
    guard::VerifiedIdentity,
    mid::rate_limit_middleware,
};
use async_trait::async_trait;
use axum::{Extension, Router, body::Body, middleware::from_fn, response::Response, routing::get};
use gate::cfg::Config;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use lim::{LimitKind, Limiter, RateLimit, RateLimitDecision};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;

struct FixedLimit {
    kind: LimitKind,
    decision: RateLimitDecision,
}

#[async_trait]
impl RateLimit for FixedLimit {
    fn kind(&self) -> LimitKind {
        self.kind
    }

    async fn check(&self, _key: &str, _cost: u64) -> lim::Result<RateLimitDecision> {
        Ok(self.decision.clone())
    }
}

fn gate(denied_limit: &str, retry_after: Option<Duration>) -> Arc<Gate> {
    let mut value = serde_json::to_value(Config::default()).unwrap();
    value["limits"]["daily"] =
        json!({"strategy":"quota_tracker", "params":{"limit":17, "period":"day"}});
    value["runtime"]["primary_concurrency"] = json!(1);
    value["http"] = json!({
        "services":{"local":{"kind":"direct_response", "status":200}},
        "policies":{"daily":{"kind":"quota", "limit":"daily"}},
        "routers":{"all":{"match":{"path":{"prefix":"/"}}, "service":"local", "policies":["daily"]}}
    });
    let mut limiter = Limiter::new();
    for (name, kind) in [
        ("ingress", LimitKind::Rate),
        ("default", LimitKind::Rate),
        ("daily", LimitKind::Quota),
    ] {
        limiter.add_limit(
            name.into(),
            Box::new(FixedLimit {
                kind,
                decision: if name == denied_limit {
                    RateLimitDecision::denied(17, 0, retry_after, None)
                } else {
                    RateLimitDecision::allowed(100, 99, None)
                },
            }),
        );
    }
    Arc::new(test_support::gate_with_limiter(
        serde_json::from_value(value).unwrap(),
        limiter,
    ))
}

fn retry_cases() -> [(Option<Duration>, &'static str); 9] {
    [
        (Some(Duration::ZERO), "0"),
        (Some(Duration::from_nanos(1)), "1"),
        (Some(Duration::from_millis(500)), "1"),
        (Some(Duration::from_secs(1)), "1"),
        (Some(Duration::from_millis(1001)), "2"),
        (Some(Duration::from_secs(60)), "60"),
        (None, "60"),
        (Some(Duration::from_secs(u64::MAX)), "18446744073709551615"),
        (Some(Duration::MAX), "18446744073709551616"),
    ]
}

async fn assert_denial(response: Response, expected: &str, prefix: &str, code: &str) {
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let headers = response.headers();
    assert_eq!(headers.get_all("retry-after").iter().count(), 1);
    let retry_after = headers["retry-after"].to_str().unwrap();
    assert!(retry_after.bytes().all(|byte| byte.is_ascii_digit()));
    assert_eq!(
        retry_after.parse::<u128>().unwrap(),
        expected.parse::<u128>().unwrap()
    );
    assert_eq!(retry_after, expected);
    assert_eq!(headers[format!("{prefix}-limit")], "17");
    assert_eq!(headers[format!("{prefix}-remaining")], "0");
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["code"],
        code
    );
}

async fn denied_handler() -> StatusCode {
    panic!("denied request reached handler")
}

#[tokio::test]
async fn gateway_ingress_rate_and_quota_denials_emit_integer_retry_after_and_keep_limit_headers() {
    for (name, prefix, scope, code) in [
        (
            "ingress",
            "x-ratelimit",
            "ingress",
            "gateway.ingress_rate_limit_exceeded",
        ),
        (
            "default",
            "x-ratelimit",
            "subject",
            "gateway.rate_limit_exceeded",
        ),
        ("daily", "x-quota", "subject", "gateway.quota_exceeded"),
    ] {
        for (duration, expected) in retry_cases() {
            let mut request = Request::new(Body::empty());
            request.extensions_mut().insert(gate(name, duration));
            request.extensions_mut().insert(TestAuthenticator {
                verify: Arc::new(|_| VerifiedIdentity::anonymous()),
            });
            let response = super::service(request).await.unwrap();
            assert_eq!(response.headers()[format!("{prefix}-scope")], scope);
            assert_denial(response, expected, prefix, code).await;
        }
    }
}

#[tokio::test]
async fn iam_admin_denials_emit_integer_retry_after_and_keep_rate_headers() {
    for (duration, expected) in retry_cases() {
        let app = Router::new()
            .route("/", get(denied_handler))
            .layer(from_fn(rate_limit_middleware))
            .layer(Extension(gate("default", duration)));
        let response = app.oneshot(Request::new(Body::empty())).await.unwrap();
        assert_denial(
            response,
            expected,
            "x-ratelimit",
            "gateway.rate_limit_exceeded",
        )
        .await;
    }
}

#[tokio::test]
async fn gateway_overload_uses_integer_retry_after_with_the_existing_ten_second_delay() {
    let gate = gate("default", None);
    let _admission = gate.resources.admit_primary().unwrap();
    let mut request = Request::new(Body::empty());
    request.extensions_mut().insert(gate);
    let response = super::service(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers().get_all("retry-after").iter().count(), 1);
    assert_eq!(response.headers()["retry-after"], "10");
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["code"],
        "gateway.overloaded"
    );
}
