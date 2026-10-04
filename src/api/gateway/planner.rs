use super::types::{ExecutionPlan, ServiceSelectionError};
use crate::err::{ErrorCode, ErrorResponse};
use gate::graph::{HttpGraph, ServiceNode, WeightedServiceNode};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

pub(super) fn build_execution_plan(
    graph: &HttpGraph,
    service_name: &str,
    seed: &str,
) -> Result<ExecutionPlan, ServiceSelectionError> {
    let service = graph.services.get(service_name).ok_or_else(|| {
        ServiceSelectionError::Internal(format!("Service '{}' not found", service_name))
    })?;

    match service {
        ServiceNode::LoadBalancer { name, upstream } => {
            let upstream_node = graph.upstreams.get(upstream).ok_or_else(|| {
                ServiceSelectionError::Internal(format!("Upstream '{}' not found", upstream))
            })?;
            Ok(ExecutionPlan::Upstream {
                service_name: name.clone(),
                internal_context: upstream_node.internal_context.clone(),
            })
        }
        ServiceNode::Weighted { services, .. } => {
            let child = choose_weighted_service(services, seed).ok_or_else(|| {
                ServiceSelectionError::Internal(format!(
                    "Weighted service '{}' has no targets",
                    service_name
                ))
            })?;
            build_execution_plan(graph, child, seed)
        }
        ServiceNode::Mirror {
            service, mirrors, ..
        } => {
            let service = Box::new(build_execution_plan(graph, service, seed)?);
            let mirrors = mirrors
                .iter()
                .filter(|mirror| mirror_selected(mirror.percent, seed, &mirror.service))
                .map(|mirror| build_execution_plan(graph, &mirror.service, seed))
                .collect::<Result<_, _>>()?;
            Ok(ExecutionPlan::Mirror { service, mirrors })
        }
        ServiceNode::Failover {
            service,
            failovers,
            on_status,
            ..
        } => {
            let services = std::iter::once(service)
                .chain(failovers)
                .map(|service| build_execution_plan(graph, service, seed))
                .collect::<Result<_, _>>()?;
            Ok(ExecutionPlan::Failover {
                services,
                on_status: on_status.clone(),
            })
        }
        ServiceNode::DirectResponse {
            status,
            headers,
            body,
            ..
        } => Ok(ExecutionPlan::DirectResponse {
            status: *status,
            headers: headers.clone(),
            body: body.clone(),
        }),
    }
}

fn mirror_selected(percent: u8, seed: &str, service: &str) -> bool {
    if percent >= 100 {
        return true;
    }

    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    service.hash(&mut hasher);
    hasher.finish() % 100 < u64::from(percent)
}

fn choose_weighted_service<'a>(services: &'a [WeightedServiceNode], seed: &str) -> Option<&'a str> {
    let total: u64 = services
        .iter()
        .map(|service| u64::from(service.weight))
        .sum();
    if total == 0 {
        return None;
    }

    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    let bucket = hasher.finish() % total;

    let mut seen = 0u64;
    for service in services {
        seen += u64::from(service.weight);
        if bucket < seen {
            return Some(service.service.as_str());
        }
    }

    services.last().map(|service| service.service.as_str())
}

pub(super) fn selection_error_response(error: ServiceSelectionError) -> ErrorResponse {
    match error {
        ServiceSelectionError::NoHealthyUpstream => {
            ErrorResponse::new(ErrorCode::GatewayNoHealthyUpstream)
        }
        ServiceSelectionError::Internal(message) => ErrorResponse::internal(message),
    }
}

#[cfg(test)]
mod tests {
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
}
