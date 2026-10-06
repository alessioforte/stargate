use crate::err::{ErrorCode, ErrorResponse};
use gate::graph::{
    HeaderValueNode, HttpGraph, InternalContextNode, ResponseBodyNode, ServiceNode,
    WeightedServiceNode,
};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

pub(in crate::api::gateway) fn build_execution_plan(
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

pub(in crate::api::gateway) fn selection_error_response(
    error: ServiceSelectionError,
) -> ErrorResponse {
    match error {
        ServiceSelectionError::NoHealthyUpstream => {
            ErrorResponse::new(ErrorCode::GatewayNoHealthyUpstream)
        }
        ServiceSelectionError::Internal(message) => ErrorResponse::internal(message),
    }
}

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub(in crate::api::gateway) enum ExecutionPlan {
    Upstream {
        service_name: String,
        internal_context: Option<InternalContextNode>,
    },
    DirectResponse {
        status: u16,
        headers: Vec<HeaderValueNode>,
        body: Option<ResponseBodyNode>,
    },
    Failover {
        services: Vec<ExecutionPlan>,
        on_status: Vec<u16>,
    },
    Mirror {
        service: Box<ExecutionPlan>,
        mirrors: Vec<ExecutionPlan>,
    },
}

#[derive(Debug)]
pub(in crate::api::gateway) enum ServiceSelectionError {
    NoHealthyUpstream,
    Internal(String),
}
