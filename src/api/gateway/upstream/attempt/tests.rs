use super::*;
use crate::api::gateway::upstream::test_support::available;
use axum::body::Body;

// Single-upstream balancer with the given open-after threshold and a long
// cooldown, so a tripped breaker stays open for the test.
fn single_upstream(service: &str, url: &str, threshold: usize) -> HashMap<String, DynLoadBalancer> {
    let upstream = lb::Upstream::with_circuit_breaker(url.to_string(), None, threshold, 3600);
    let balancer: DynLoadBalancer =
        lb::BaseLoadBalancer::new(lb::RoundRobin::new(), vec![upstream]);
    let mut map = HashMap::new();
    map.insert(service.to_string(), balancer);
    map
}

fn response(status: u16) -> Result<Response, ErrorResponse> {
    Ok(Response::builder()
        .status(status)
        .body(Body::empty())
        .unwrap())
}

fn transport_error() -> Result<Response, ErrorResponse> {
    Err(ErrorResponse::new(ErrorCode::UpstreamConnectionFailed))
}

#[test]
fn dispatch_attempts_count_only_network_calls_and_reset_by_kind() {
    let direct = SelectedService::DirectResponse {
        status: 200,
        headers: Vec::new(),
        body: None,
    };
    let upstream = SelectedService::Upstream {
        service_name: "orders".to_owned(),
        upstream_base_url: "http://orders.test".to_owned(),
        internal_context: None,
    };
    let mut primary_counter = 0;
    let mut shadow_counter = 0;

    assert!(
        next_dispatch_attempt(&direct, DispatchKind::Primary, &mut primary_counter)
            .unwrap()
            .is_none()
    );
    let primary_one = next_dispatch_attempt(&upstream, DispatchKind::Primary, &mut primary_counter)
        .unwrap()
        .unwrap();
    let primary_two = next_dispatch_attempt(&upstream, DispatchKind::Primary, &mut primary_counter)
        .unwrap()
        .unwrap();
    let shadow_one = next_dispatch_attempt(&upstream, DispatchKind::Shadow, &mut shadow_counter)
        .unwrap()
        .unwrap();

    assert_eq!(primary_one.number, 1);
    assert_eq!(primary_two.number, 2);
    assert_eq!(shadow_one.number, 1);
    assert_eq!(primary_one.kind, DispatchKind::Primary);
    assert_eq!(shadow_one.kind, DispatchKind::Shadow);
}

#[test]
fn transport_error_then_success_toggles_availability() {
    let balancers = single_upstream("svc", "http://up", 1);
    record_upstream_health(&balancers, "svc", "http://up", &transport_error());
    assert!(
        !available(&balancers, "svc"),
        "transport error should eject"
    );
    record_upstream_health(&balancers, "svc", "http://up", &response(200));
    assert!(available(&balancers, "svc"), "success should restore");
}

#[test]
fn infrastructure_5xx_ejects_but_app_5xx_does_not() {
    let balancers = single_upstream("svc", "http://up", 1);
    record_upstream_health(&balancers, "svc", "http://up", &response(503));
    assert!(!available(&balancers, "svc"), "503 is an upstream failure");

    let balancers = single_upstream("svc", "http://up", 1);
    record_upstream_health(&balancers, "svc", "http://up", &response(500));
    assert!(
        available(&balancers, "svc"),
        "500 is an application error, breaker stays closed"
    );
}

#[test]
fn unknown_service_is_a_noop() {
    let balancers = single_upstream("svc", "http://up", 1);
    record_upstream_health(&balancers, "missing", "http://up", &transport_error());
    assert!(available(&balancers, "svc"));
}
