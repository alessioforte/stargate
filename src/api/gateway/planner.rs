use super::types::{DynLoadBalancer, ExecutionPlan, SelectedService, ServiceSelectionError};
use crate::err::{ErrorResponse, HttpError};
use gate::graph::{HttpGraph, ServiceNode, WeightedServiceNode};
use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

pub(super) fn build_execution_plan(
    graph: &HttpGraph,
    balancers: &HashMap<String, DynLoadBalancer>,
    service_name: &str,
    ctx: &lb::RequestContext<'_>,
    seed: &str,
) -> Result<ExecutionPlan, ServiceSelectionError> {
    let service = graph.services.get(service_name).ok_or_else(|| {
        ServiceSelectionError::Internal(format!("Service '{}' not found", service_name))
    })?;

    match service {
        ServiceNode::LoadBalancer { name, .. } => {
            let lb = balancers.get(name).ok_or_else(|| {
                ServiceSelectionError::Internal(format!("Load balancer '{}' not found", name))
            })?;

            let upstream = lb
                .select(ctx)
                .ok_or(ServiceSelectionError::NoHealthyUpstream)?;

            Ok(ExecutionPlan {
                attempts: vec![SelectedService::Upstream {
                    service_name: name.clone(),
                    upstream_base_url: upstream.base_url.clone(),
                }],
                ..ExecutionPlan::default()
            })
        }
        ServiceNode::Weighted { services, .. } => {
            let child = choose_weighted_service(services, seed).ok_or_else(|| {
                ServiceSelectionError::Internal(format!(
                    "Weighted service '{}' has no targets",
                    service_name
                ))
            })?;
            build_execution_plan(graph, balancers, child, ctx, seed)
        }
        ServiceNode::Mirror {
            service, mirrors, ..
        } => {
            let mut plan = build_execution_plan(graph, balancers, service, ctx, seed)?;
            for mirror in mirrors {
                if !mirror_selected(mirror.percent, seed, &mirror.service) {
                    continue;
                }
                match build_execution_plan(graph, balancers, &mirror.service, ctx, seed) {
                    Ok(mirror_plan) => plan.mirrors.push(mirror_plan),
                    Err(error) => tracing::warn!(
                        service = mirror.service.as_str(),
                        error = %selection_error_message(&error),
                        "Mirror target skipped"
                    ),
                }
            }
            Ok(plan)
        }
        ServiceNode::Failover {
            service,
            failovers,
            on_status,
            ..
        } => {
            let mut plan = ExecutionPlan::default();
            append_available_plan(&mut plan, graph, balancers, service, ctx, seed)?;
            for failover in failovers {
                append_available_plan(&mut plan, graph, balancers, failover, ctx, seed)?;
            }

            if plan.attempts.is_empty() {
                return Err(ServiceSelectionError::NoHealthyUpstream);
            }

            plan.failover_on_status.extend(on_status.iter().copied());
            Ok(plan)
        }
        ServiceNode::DirectResponse {
            status,
            headers,
            body,
            ..
        } => Ok(ExecutionPlan {
            attempts: vec![SelectedService::DirectResponse {
                status: *status,
                headers: headers.clone(),
                body: body.clone(),
            }],
            ..ExecutionPlan::default()
        }),
    }
}

fn append_available_plan(
    target: &mut ExecutionPlan,
    graph: &HttpGraph,
    balancers: &HashMap<String, DynLoadBalancer>,
    service_name: &str,
    ctx: &lb::RequestContext<'_>,
    seed: &str,
) -> Result<(), ServiceSelectionError> {
    match build_execution_plan(graph, balancers, service_name, ctx, seed) {
        Ok(plan) => {
            target.attempts.extend(plan.attempts);
            target.failover_on_status.extend(plan.failover_on_status);
            target.mirrors.extend(plan.mirrors);
            Ok(())
        }
        Err(ServiceSelectionError::NoHealthyUpstream) => Ok(()),
        Err(error) => Err(error),
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

fn selection_error_message(error: &ServiceSelectionError) -> String {
    match error {
        ServiceSelectionError::NoHealthyUpstream => "no healthy upstream".to_string(),
        ServiceSelectionError::Internal(message) => message.clone(),
    }
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
        ServiceSelectionError::NoHealthyUpstream => ErrorResponse::from(
            HttpError::ServiceUnavailable("No healthy upstream available".to_string()),
        ),
        ServiceSelectionError::Internal(message) => {
            ErrorResponse::from(HttpError::InternalServerError(message))
        }
    }
}
