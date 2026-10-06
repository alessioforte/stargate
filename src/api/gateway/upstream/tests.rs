use super::*;
use crate::api::gateway::execution::{execute_plan_from_replay, plan::ExecutionPlan};
use crate::api::gateway::upstream::test_support::{replay_request, replay_state};
use crate::etc::request_context::INTERNAL_CONTEXT_HEADER;
use ctx::DispatchKind;
use gate::graph::{HeaderValueNode, InternalContextNode};

fn runtime() -> Arc<RuntimeSnapshot> {
    let config = gate::cfg::RuntimeConfig::from_yaml_str(
        r#"
schema: stargate/v1
ingress:
  limit: ingress
  timeout: 250ms
limits:
  default:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 1s
  ingress:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 100ms
http:
  upstreams:
    test:
      targets:
        - url: http://127.0.0.1:1
  services:
    unreachable:
      kind: load_balancer
      upstream: test
    orders:
      kind: load_balancer
      upstream: test
"#,
    )
    .unwrap();
    crate::etc::gate::test_support::runtime(config)
}

#[test]
fn websocket_uri_maps_http_schemes_and_preserves_the_rest() {
    assert_eq!(
        websocket_uri("https://upstream.example/socket?token=abc"),
        "wss://upstream.example/socket?token=abc"
    );
    assert_eq!(
        websocket_uri("ws://upstream.example/socket"),
        "ws://upstream.example/socket"
    );
    assert_eq!(
        websocket_uri("wss://upstream.example/socket"),
        "wss://upstream.example/socket"
    );
}

#[tokio::test]
async fn direct_response_remains_local_and_has_no_internal_context() {
    let selected = SelectedService::DirectResponse {
        status: 202,
        headers: vec![HeaderValueNode {
            name: "x-direct".to_owned(),
            value: "yes".to_owned(),
        }],
        body: None,
    };
    let state = RequestState {
        execution: crate::api::gateway::lifecycle::Execution::default(),
        client_ip: "127.0.0.1".into(),
        load_balancer_key: None,
        original_path: "/direct".to_owned(),
        path: "/direct".to_owned(),
        query: String::new(),
        preserve_host: false,
        response_headers: Default::default(),
        propagation_draft: None,
        internal_context_runtime: None,
    };
    let request = Request::builder().body(Body::empty()).unwrap();

    let response = execute_selected_with_request(&runtime(), &selected, request, &state)
        .await
        .unwrap();

    assert_eq!(response.status(), 202);
    assert_eq!(response.headers()["x-direct"], "yes");
    assert!(response.headers().get(INTERNAL_CONTEXT_HEADER).is_none());
}

#[tokio::test]
async fn status_failover_advances_to_the_next_plan_entry() {
    let plan = ExecutionPlan::Failover {
        services: vec![
            ExecutionPlan::DirectResponse {
                status: 503,
                headers: Vec::new(),
                body: None,
            },
            ExecutionPlan::DirectResponse {
                status: 204,
                headers: Vec::new(),
                body: None,
            },
        ],
        on_status: vec![503],
    };

    let response = execute_plan_from_replay(
        &runtime(),
        &plan,
        &replay_request(),
        &replay_state(),
        DispatchKind::Primary,
    )
    .await
    .unwrap();

    assert_eq!(response.status(), 204);
}

#[tokio::test]
async fn transport_failure_advances_even_without_status_rules() {
    let plan = ExecutionPlan::Failover {
        services: vec![
            ExecutionPlan::Upstream {
                service_name: "unreachable".to_owned(),
                internal_context: None,
            },
            ExecutionPlan::DirectResponse {
                status: 204,
                headers: Vec::new(),
                body: None,
            },
        ],
        on_status: Vec::new(),
    };

    let response = execute_plan_from_replay(
        &runtime(),
        &plan,
        &replay_request(),
        &replay_state(),
        DispatchKind::Primary,
    )
    .await
    .unwrap();

    assert_eq!(response.status(), 204);
}

#[tokio::test]
async fn internal_context_failure_never_fails_over() {
    let plan = ExecutionPlan::Failover {
        services: vec![
            ExecutionPlan::Upstream {
                service_name: "orders".to_owned(),
                internal_context: Some(InternalContextNode {
                    audience: "urn:stargate:service:orders".to_owned(),
                }),
            },
            ExecutionPlan::DirectResponse {
                status: 204,
                headers: Vec::new(),
                body: None,
            },
        ],
        on_status: Vec::new(),
    };

    let error = execute_plan_from_replay(
        &runtime(),
        &plan,
        &replay_request(),
        &replay_state(),
        DispatchKind::Primary,
    )
    .await
    .unwrap_err();

    assert_eq!(error.status, ::http::StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn local_errors_and_shadow_attempts_preserve_upstream_health() {
    for replay in [false, true] {
        for signed in [false, true] {
            for kind in [DispatchKind::Primary, DispatchKind::Shadow] {
                if !replay && kind == DispatchKind::Shadow {
                    continue; // Live requests are always primary attempts.
                }
                let mut runtime = runtime();
                let url = "http://127.0.0.1:1";
                let target = lb::Upstream::with_circuit_breaker(url.to_owned(), None, 1, 3600);
                let circuit = target.circuit_breaker.clone();
                Arc::get_mut(&mut runtime).unwrap().core.balancers.insert(
                    "orders".to_owned(),
                    lb::BaseLoadBalancer::new(lb::RoundRobin::new(), vec![target]),
                );
                let selected = SelectedService::Upstream {
                    service_name: "orders".to_owned(),
                    upstream_base_url: url.to_owned(),
                    internal_context: signed.then(|| InternalContextNode {
                        audience: "urn:orders".to_owned(),
                    }),
                };
                let state = replay_state(); // No draft: signed preparation fails locally.
                let result = if replay {
                    execute_selected_from_replay(
                        &runtime,
                        &selected,
                        &replay_request(),
                        &state,
                        Some(DispatchAttempt { kind, number: 1 }),
                    )
                    .await
                } else {
                    execute_selected_with_request(
                        &runtime,
                        &selected,
                        Request::new(Body::empty()),
                        &state,
                    )
                    .await
                };
                assert_eq!(
                    result.unwrap_err().code,
                    if signed {
                        ErrorCode::GatewayRequestPreparationFailed
                    } else {
                        ErrorCode::UpstreamConnectionFailed
                    }
                );
                assert_eq!(
                    circuit.state(),
                    u8::from(!signed && kind == DispatchKind::Primary),
                    "replay={replay}, signed={signed}, kind={kind:?}"
                );
            }
        }
    }
}
