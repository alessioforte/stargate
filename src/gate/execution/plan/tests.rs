use super::*;
use gate::cfg::RuntimeConfig;

#[test]
fn selected_leaf_carries_its_compiled_audience_not_its_target_url() {
    let config = RuntimeConfig::from_yaml_str(
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
    orders-pool:
      targets:
        - url: http://orders.internal:8080
      internal_context:
        audience: urn:stargate:service:orders
  services:
    orders-leaf:
      kind: load_balancer
      upstream: orders-pool
"#,
    )
    .unwrap();
    let graph = &config.compiled().http;
    let plan = build_execution_plan(graph, "orders-leaf", "request-id").unwrap();
    let ExecutionPlan::Upstream {
        service_name,
        internal_context,
    } = &plan
    else {
        panic!("expected an upstream leaf");
    };
    assert_eq!(service_name, "orders-leaf");
    assert_eq!(
        internal_context
            .as_ref()
            .map(|context| context.audience.as_str()),
        Some("urn:stargate:service:orders")
    );
}

#[test]
fn failover_and_mirror_plans_retain_each_leaf_audience() {
    let config = RuntimeConfig::from_yaml_str(
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
    primary-pool:
      targets:
        - url: http://primary.internal
      internal_context:
        audience: urn:stargate:service:primary
    fallback-pool:
      targets:
        - url: http://fallback.internal
      internal_context:
        audience: urn:stargate:service:fallback
    shadow-pool:
      targets:
        - url: http://shadow.internal
      internal_context:
        audience: urn:stargate:service:shadow
  services:
    primary-leaf:
      kind: load_balancer
      upstream: primary-pool
    fallback-leaf:
      kind: load_balancer
      upstream: fallback-pool
    shadow-leaf:
      kind: load_balancer
      upstream: shadow-pool
    primary-failover:
      kind: failover
      service: primary-leaf
      failovers:
        - fallback-leaf
      on_status:
        - 503
    shadow-failover:
      kind: failover
      service: shadow-leaf
      failovers:
        - fallback-leaf
    root:
      kind: mirror
      service: primary-failover
      mirrors:
        - service: shadow-failover
          percent: 100
"#,
    )
    .unwrap();
    let graph = &config.compiled().http;
    let plan = build_execution_plan(graph, "root", "request-id").unwrap();
    let ExecutionPlan::Mirror { service, mirrors } = &plan else {
        panic!("expected mirror boundary")
    };
    fn audiences(plan: &ExecutionPlan) -> Vec<&str> {
        let ExecutionPlan::Failover { services, .. } = plan else {
            panic!("expected failover boundary")
        };
        services
            .iter()
            .map(|service| {
                let ExecutionPlan::Upstream {
                    internal_context, ..
                } = service
                else {
                    panic!("expected leaf")
                };
                internal_context.as_ref().unwrap().audience.as_str()
            })
            .collect::<Vec<_>>()
    }
    assert_eq!(
        audiences(service),
        vec![
            "urn:stargate:service:primary",
            "urn:stargate:service:fallback"
        ]
    );
    assert_eq!(mirrors.len(), 1);
    assert_eq!(
        audiences(&mirrors[0]),
        vec![
            "urn:stargate:service:shadow",
            "urn:stargate:service:fallback"
        ]
    );
}

#[test]
fn weighted_selection_keeps_the_chosen_leaf_audience() {
    let config = RuntimeConfig::from_yaml_str(
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
    blue-pool:
      targets:
        - url: http://blue.internal
      internal_context:
        audience: urn:stargate:service:blue
    green-pool:
      targets:
        - url: http://green.internal
      internal_context:
        audience: urn:stargate:service:green
  services:
    blue:
      kind: load_balancer
      upstream: blue-pool
    green:
      kind: load_balancer
      upstream: green-pool
    weighted:
      kind: weighted
      services:
        - name: blue
          weight: 1
        - name: green
          weight: 1
"#,
    )
    .unwrap();
    let graph = &config.compiled().http;
    let mut selected_services = std::collections::HashSet::new();

    for seed in 0..64 {
        let plan = build_execution_plan(graph, "weighted", &seed.to_string()).unwrap();
        let ExecutionPlan::Upstream {
            service_name,
            internal_context,
            ..
        } = &plan
        else {
            panic!("weighted leaf must be an upstream");
        };
        let expected = if service_name == "blue" {
            "urn:stargate:service:blue"
        } else {
            assert_eq!(service_name, "green");
            "urn:stargate:service:green"
        };
        assert_eq!(internal_context.as_ref().unwrap().audience, expected);
        selected_services.insert(service_name.clone());
    }

    assert_eq!(selected_services.len(), 2);
}
