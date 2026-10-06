use crate::api::gateway::execution::{
    execute_plan_from_replay, plan::ExecutionPlan, replay::ReplayEligibility,
};
use crate::api::gateway::tests::replay_support::{
    Outcome, Upstream, config, plan, planned, replay, state,
};
use crate::{err::ErrorCode, etc::gate::test_support};
use gate::cfg::RuntimeConfig;
use http::{Method, StatusCode};
use http_body_util::BodyExt;
use hyper::body::Bytes;
use std::sync::Arc;

#[test]
fn replay_eligibility_is_conservative() {
    for method in [
        Method::GET,
        Method::HEAD,
        Method::OPTIONS,
        Method::TRACE,
        Method::PUT,
        Method::DELETE,
    ] {
        let eligibility = ReplayEligibility::for_method(&method);
        assert_eq!(eligibility, ReplayEligibility::Idempotent);
    }
    for method in [
        Method::POST,
        Method::PATCH,
        Method::CONNECT,
        Method::from_bytes(b"CUSTOM").unwrap(),
        Method::from_bytes(b"get").unwrap(),
    ] {
        let eligibility = ReplayEligibility::for_method(&method);
        assert_eq!(eligibility, ReplayEligibility::SingleDispatch);
    }
}

#[tokio::test]
async fn buffered_mutations_never_repeat_after_committing_and_losing_headers() {
    for method in [
        Method::POST,
        Method::PATCH,
        Method::from_bytes(b"CUSTOM").unwrap(),
    ] {
        let primary = Upstream::start(Outcome::CommitThenClose).await;
        let backup = Upstream::start(Outcome::Status(200)).await;
        let runtime =
            test_support::runtime(RuntimeConfig::from_raw(config(&primary, &backup)).unwrap());
        let error = execute_plan_from_replay(
            &runtime,
            &plan(),
            &replay(method),
            &state(&runtime),
            ctx::DispatchKind::Primary,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::UpstreamConnectionFailed);
        assert_eq!(primary.records().len(), 1);
        assert!(backup.records().is_empty());
    }
}

#[tokio::test]
async fn buffered_mutations_return_the_primary_status_without_disposal_or_failover() {
    for kind in [ctx::DispatchKind::Primary, ctx::DispatchKind::Shadow] {
        let primary = Upstream::start(Outcome::Status(503)).await;
        let backup = Upstream::start(Outcome::Status(200)).await;
        let runtime =
            test_support::runtime(RuntimeConfig::from_raw(config(&primary, &backup)).unwrap());
        let response = execute_plan_from_replay(
            &runtime,
            &plan(),
            &replay(Method::POST),
            &state(&runtime),
            kind,
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "upstream response"
        );
        assert_eq!(primary.records().len(), 1);
        assert!(backup.records().is_empty());
    }
}

#[tokio::test]
async fn idempotent_operations_fail_over_on_transport_and_configured_status() {
    for method in [
        Method::GET,
        Method::HEAD,
        Method::OPTIONS,
        Method::TRACE,
        Method::PUT,
        Method::DELETE,
    ] {
        for outcome in [Outcome::CommitThenClose, Outcome::Status(503)] {
            let primary = Upstream::start(outcome).await;
            let backup = Upstream::start(Outcome::Status(200)).await;
            let runtime =
                test_support::runtime(RuntimeConfig::from_raw(config(&primary, &backup)).unwrap());
            let mut replay = replay(method.clone());
            if method != Method::PUT && method != Method::DELETE {
                replay.body = Bytes::new();
            }
            let response = execute_plan_from_replay(
                &runtime,
                &plan(),
                &replay,
                &state(&runtime),
                ctx::DispatchKind::Primary,
            )
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            drop(response);
            for upstream in [&primary, &backup] {
                let records = upstream.records();
                assert_eq!(records.len(), 1);
                assert_eq!(records[0].method, method);
                assert_eq!(records[0].body, replay.body);
                assert_eq!(records[0].headers["idempotency-key"], "the-same-key");
            }
        }
    }
}

#[tokio::test]
async fn request_preparation_and_signing_failures_do_not_advance_the_plan() {
    let primary = Upstream::start(Outcome::Status(200)).await;
    let backup = Upstream::start(Outcome::Status(200)).await;
    for failure in ["uri", "relative_uri", "version", "signing"] {
        let mut runtime =
            test_support::runtime(RuntimeConfig::from_raw(config(&primary, &backup)).unwrap());
        let mut plan = plan();
        if failure == "signing" {
            let ExecutionPlan::Failover { services, .. } = &mut plan else {
                unreachable!()
            };
            let ExecutionPlan::Upstream {
                internal_context, ..
            } = &mut services[0]
            else {
                unreachable!()
            };
            *internal_context = Some(gate::graph::InternalContextNode {
                audience: "urn:primary".into(),
            });
        } else if failure == "uri" || failure == "relative_uri" {
            let url = if failure == "uri" {
                "http://["
            } else {
                "relative"
            };
            Arc::get_mut(&mut runtime).unwrap().core.balancers.insert(
                "primary".into(),
                lb::BaseLoadBalancer::new(
                    lb::RoundRobin::new(),
                    vec![lb::Upstream::new(url.into(), None)],
                ),
            );
        }
        let mut replay = replay(Method::PUT);
        if failure == "version" {
            replay.version = http::Version::HTTP_3;
        }
        let error = execute_plan_from_replay(
            &runtime,
            &plan,
            &replay,
            &state(&runtime),
            ctx::DispatchKind::Primary,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::GatewayRequestPreparationFailed);
        assert!(primary.records().is_empty());
        assert!(backup.records().is_empty());
    }
}

#[tokio::test]
async fn eligible_failover_mints_a_fresh_context_for_each_target_and_attempt() {
    for kind in [ctx::DispatchKind::Primary, ctx::DispatchKind::Shadow] {
        verify_nested_failover_contexts(kind).await;
    }
}

async fn verify_nested_failover_contexts(kind: ctx::DispatchKind) {
    use crate::etc::{
        auth::identity::VerifiedIdentity,
        internal_context::InternalContextRuntime,
        request_context::{INTERNAL_CONTEXT_HEADER, PropagationDraft, RequestContext},
    };
    use ctx::{
        ContextSigner, ContextVerifier, ExpectedRequest, SignerConfig, StaticKeyResolver,
        VerifierConfig,
    };
    const ISSUER: &str = "https://stargate.test/internal-context";
    const KEY_ID: &str = "replay-test";
    const REQUEST_ID: &str = "01JZ000000000000000000000R";
    let primary = Upstream::start(Outcome::Status(503)).await;
    let backup = Upstream::start(Outcome::Status(200)).await;
    let mut runtime =
        test_support::runtime(RuntimeConfig::from_raw(config(&primary, &backup)).unwrap());
    Arc::get_mut(&mut runtime).unwrap().core.balancers.insert(
        "unavailable".into(),
        lb::BaseLoadBalancer::new(lb::RoundRobin::new(), Vec::new()),
    );
    let signer = ContextSigner::from_rsa_pem(
        SignerConfig::new(ISSUER, KEY_ID, 30).unwrap(),
        crate::etc::internal_context::test_support::PRIVATE_KEY,
    )
    .unwrap();
    let request_context =
        RequestContext::new(REQUEST_ID.into(), chrono::Utc::now(), None, None, None);
    let mut state = state(&runtime);
    state.propagation_draft = Some(Arc::new(
        PropagationDraft::build(
            &VerifiedIdentity::anonymous(),
            &request_context,
            "PUT",
            "/orders",
            "/orders",
            ctx::RouteContext {
                router: "entry".to_owned(),
                service: "entry".to_owned(),
                policy_revision: None,
            },
        )
        .unwrap(),
    ));
    state.internal_context_runtime = Some(InternalContextRuntime::from_signer_for_test(signer));
    let audiences = ["urn:primary", "urn:backup"];
    let mut plan = plan();
    let ExecutionPlan::Failover { services, .. } = &mut plan else {
        unreachable!()
    };
    for (selected, audience) in services.iter_mut().zip(audiences) {
        let ExecutionPlan::Upstream {
            internal_context, ..
        } = selected
        else {
            unreachable!()
        };
        *internal_context = Some(gate::graph::InternalContextNode {
            audience: audience.into(),
        });
    }
    let plan = ExecutionPlan::Failover {
        services: vec![
            ExecutionPlan::DirectResponse {
                status: 503,
                headers: Vec::new(),
                body: None,
            },
            ExecutionPlan::Failover {
                services: vec![planned("unavailable"), plan],
                on_status: vec![404],
            },
        ],
        on_status: vec![503],
    };
    let mut replay = replay(Method::PUT);
    replay.headers.insert(
        INTERNAL_CONTEXT_HEADER.clone(),
        "stale-client-token".parse().unwrap(),
    );
    let response = execute_plan_from_replay(&runtime, &plan, &replay, &state, kind)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    drop(response);
    let mut dispatch_ids = std::collections::HashSet::new();
    for (index, (upstream, audience)) in [&primary, &backup].into_iter().zip(audiences).enumerate()
    {
        let records = upstream.records();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0]
                .headers
                .get_all(&INTERNAL_CONTEXT_HEADER)
                .iter()
                .count(),
            1
        );
        let token = records[0].headers[&INTERNAL_CONTEXT_HEADER]
            .to_str()
            .unwrap();
        let resolver = StaticKeyResolver::from_rsa_pem(
            KEY_ID,
            crate::etc::internal_context::test_support::PUBLIC_KEY,
        )
        .unwrap();
        let verifier = ContextVerifier::new(
            VerifierConfig::new(ISSUER, audience, 5).unwrap(),
            Arc::new(resolver),
        );
        let trusted = verifier
            .verify(
                token,
                ExpectedRequest {
                    method: "PUT",
                    encoded_path: "/orders",
                    request_id: REQUEST_ID,
                },
            )
            .unwrap();
        assert_eq!(trusted.context().dispatch.kind, kind);
        assert_eq!(trusted.context().dispatch.attempt, index as u16 + 1);
        assert!(dispatch_ids.insert(trusted.dispatch_id().to_owned()));
    }
    assert_eq!(dispatch_ids.len(), 2);
}

#[cfg(feature = "memory")]
mod ingress {
    use super::*;
    use crate::etc::gate::Gate;
    use axum::{body::Body, response::Response};
    use futures_util::stream;
    use http::Request;
    use std::{io, time::Duration};

    fn request(gate: &Arc<Gate>, method: Method, body: Body) -> Request<Body> {
        let mut request = Request::builder()
            .method(method)
            .uri("/orders")
            .header("idempotency-key", "the-same-key")
            .body(body)
            .unwrap();
        request.extensions_mut().insert(gate.clone());
        request
    }

    async fn send(request: Request<Body>) -> Response {
        tokio::time::timeout(
            Duration::from_secs(3),
            crate::api::gateway::service(request),
        )
        .await
        .unwrap()
        .unwrap()
    }

    async fn assert_resources_returned(gate: &Gate) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while gate.resources.available().2 != 32 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn mutations_stream_past_the_replay_cap_without_reserving_failover_storage() {
        for method in [Method::POST, Method::PATCH] {
            for declared_length in [false, true] {
                let primary = Upstream::start(Outcome::Status(200)).await;
                let backup = Upstream::start(Outcome::Status(200)).await;
                let gate = Arc::new(test_support::gate(config(&primary, &backup)));
                let occupied = gate.resources.reserve_replay(32).unwrap();
                let body = Body::from_stream(stream::iter([
                    Ok::<_, io::Error>(Bytes::from_static(b"1234567890")),
                    Ok(Bytes::from_static(b"abcdefghij")),
                ]));
                let mut req = request(&gate, method.clone(), body);
                if declared_length {
                    req.headers_mut()
                        .insert("content-length", "20".parse().unwrap());
                }
                let response = send(req).await;
                assert_eq!(response.status(), StatusCode::OK);
                drop(response);
                let records = primary.records();
                assert_eq!(records.len(), 1);
                assert_eq!(records[0].body, "1234567890abcdefghij");
                assert!(backup.records().is_empty());
                drop(occupied);
                assert_resources_returned(&gate).await;
            }
        }
    }

    #[tokio::test]
    async fn streaming_mutation_commits_only_once_when_the_connection_closes() {
        for method in [Method::POST, Method::PATCH] {
            let primary = Upstream::start(Outcome::CommitThenClose).await;
            let backup = Upstream::start(Outcome::Status(200)).await;
            let gate = Arc::new(test_support::gate(config(&primary, &backup)));
            let response = send(request(&gate, method, Body::from("larger-than-cap"))).await;
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
            assert_eq!(primary.records().len(), 1);
            assert!(backup.records().is_empty());
            assert_resources_returned(&gate).await;
        }
    }

    #[tokio::test]
    async fn mutations_can_select_an_available_fallback_before_dispatch() {
        let primary = Upstream::start(Outcome::Status(200)).await;
        let backup = Upstream::start(Outcome::Status(200)).await;
        let gate = Arc::new(test_support::gate(config(&primary, &backup)));
        gate.snapshot().core.balancers["primary"].mark_dead(&primary.url);
        let response = send(request(&gate, Method::POST, Body::from("larger-than-cap"))).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(primary.records().is_empty());
        assert_eq!(backup.records().len(), 1);
        assert_eq!(backup.records()[0].method, Method::POST);
        assert_eq!(backup.records()[0].body, "larger-than-cap");
        drop(response);
        assert_resources_returned(&gate).await;
    }

    #[tokio::test]
    async fn client_body_errors_never_trigger_fallback_or_upstream_health_failures() {
        for method in [Method::POST, Method::PUT] {
            let primary = Upstream::start(Outcome::Status(200)).await;
            let backup = Upstream::start(Outcome::Status(200)).await;
            let gate = Arc::new(test_support::gate(config(&primary, &backup)));
            let body = Body::from_stream(stream::iter([
                Ok(Bytes::from_static(b"part")),
                Err(io::Error::other("client body failed")),
            ]));
            let response = send(request(&gate, method, body)).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&body).unwrap()["code"],
                "gateway.request_body_failed"
            );
            assert!(primary.records().is_empty());
            assert!(backup.records().is_empty());
            assert!(
                gate.snapshot().core.balancers["primary"]
                    .select(&lb::RequestContext {
                        client_ip: "unknown",
                        path: "/orders",
                        method: "GET",
                        key: None
                    })
                    .is_some()
            );
            assert_resources_returned(&gate).await;
        }
    }

    #[tokio::test]
    async fn mirrors_still_buffer_mutations_but_do_not_enable_primary_retries() {
        let primary = Upstream::start(Outcome::Status(503)).await;
        let backup = Upstream::start(Outcome::Status(200)).await;
        let shadow = Upstream::start(Outcome::Status(200)).await;
        let mut config = config(&primary, &backup);
        config.http.upstreams.insert(
            "shadow".into(),
            serde_json::from_value(serde_json::json!({"targets":[{"url":shadow.url}]})).unwrap(),
        );
        config.http.services.insert(
            "shadow".into(),
            gate::cfg::Service::LoadBalancer {
                upstream: "shadow".into(),
            },
        );
        config.http.services.insert("mirror".into(), serde_json::from_value(serde_json::json!({"kind":"mirror","service":"entry","mirrors":[{"service":"shadow","percent":100}]})).unwrap());
        config.http.routers.get_mut("entry").unwrap().service = "mirror".into();
        let gate = Arc::new(test_support::gate(config));
        let response = send(request(&gate, Method::POST, Body::from("body"))).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        drop(response);
        tokio::time::timeout(Duration::from_secs(1), async {
            while shadow.records().is_empty()
                || gate.resources.available().1 != gate.resources.budgets.mirror_concurrency
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(primary.records().len(), 1);
        assert!(backup.records().is_empty());
        assert_eq!(shadow.records().len(), 1);
        assert_eq!(shadow.records()[0].body, "body");
        assert_resources_returned(&gate).await;

        let response = send(request(
            &gate,
            Method::POST,
            Body::from_stream(stream::iter([Ok::<_, io::Error>(Bytes::from_static(
                b"larger-than-cap",
            ))])),
        ))
        .await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(primary.records().len(), 1);
        assert_eq!(shadow.records().len(), 1);
        assert_resources_returned(&gate).await;
    }
}
