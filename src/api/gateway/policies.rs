use super::limits::{OrgScopedLimit, OrgScopedQuota, SelectedLimitPolicies, apply_limits};
use super::types::AuthKind;
use crate::err::{ErrorResponse, HttpError};
use crate::etc::{ac::access_control, reqctx, sub::Subject, telemetry};
use ::http::{HeaderMap, Request};
use axum::body::Body;
use gate::{
    Gate,
    cfg::{AuthStrategy, EnvProfile, LimitScope},
    graph::{HttpGraph, PolicyNode, RouterNode},
};
use std::sync::Arc;

#[allow(clippy::too_many_arguments)]
pub async fn apply_policies(
    graph: &HttpGraph,
    router: &RouterNode,
    gate: &Arc<Gate>,
    req: &mut Request<Body>,
    auth_kind: Option<AuthKind>,
    subject: Option<&Subject>,
    client_ip: &str,
    response_headers: &mut HeaderMap,
) -> Result<(), ErrorResponse> {
    let mut selected_limits = SelectedLimitPolicies::default();

    for policy_name in &router.policies {
        let policy = graph.policies.get(policy_name).ok_or_else(|| {
            ErrorResponse::from(HttpError::InternalServerError(format!(
                "Policy '{}' not found",
                policy_name
            )))
        })?;

        match policy {
            PolicyNode::Auth { strategies } => {
                let Some(kind) = auth_kind else {
                    telemetry::record_gateway_policy("auth", "denied");
                    return Err(ErrorResponse::from(HttpError::Unauthorized(
                        "Unauthorized".to_string(),
                    )));
                };
                if !auth_strategy_allowed(strategies, kind) {
                    telemetry::record_gateway_policy("auth", "denied");
                    return Err(ErrorResponse::from(HttpError::Unauthorized(
                        "Unauthorized".to_string(),
                    )));
                }
                telemetry::record_gateway_policy("auth", "allowed");
            }
            PolicyNode::AccessControl { resource, env } => {
                let Some(subject) = subject else {
                    telemetry::record_gateway_policy("access_control", "denied");
                    return Err(ErrorResponse::from(HttpError::Unauthorized(
                        "Unauthorized".to_string(),
                    )));
                };

                let profile = env.unwrap_or(EnvProfile::Geo);
                let pe = gate.policy_engine.load();
                let env = reqctx::build_env_from(req.extensions_mut(), profile);
                let allowed = access_control(&pe, subject, &env, resource);

                if !allowed {
                    telemetry::record_gateway_policy("access_control", "denied");
                    return Err(ErrorResponse::from(HttpError::Forbidden(
                        "Forbidden".to_string(),
                    )));
                }
                telemetry::record_gateway_policy("access_control", "allowed");
            }
            PolicyNode::RateLimit {
                limit,
                scope,
                on_missing,
            } => {
                let duplicate = match scope {
                    LimitScope::Subject => selected_limits
                        .subject_rate
                        .replace(limit.clone())
                        .is_some(),
                    LimitScope::Org => selected_limits
                        .org_rate
                        .replace(OrgScopedLimit {
                            limit: limit.clone(),
                            on_missing: *on_missing,
                        })
                        .is_some(),
                };
                if duplicate {
                    return Err(ErrorResponse::from(HttpError::InternalServerError(
                        "Multiple rate_limit policies for the same scope are not supported"
                            .to_string(),
                    )));
                }
            }
            PolicyNode::Quota {
                limit,
                cost,
                scope,
                on_missing,
            } => {
                let duplicate = match scope {
                    LimitScope::Subject => selected_limits
                        .subject_quota
                        .replace((limit.clone(), *cost))
                        .is_some(),
                    LimitScope::Org => selected_limits
                        .org_quota
                        .replace(OrgScopedQuota {
                            limit: limit.clone(),
                            cost: *cost,
                            on_missing: *on_missing,
                        })
                        .is_some(),
                };
                if duplicate {
                    return Err(ErrorResponse::from(HttpError::InternalServerError(
                        "Multiple quota policies for the same scope are not supported".to_string(),
                    )));
                }
            }
        }
    }

    apply_limits(gate, subject, client_ip, response_headers, selected_limits).await
}

fn auth_strategy_allowed(strategies: &[AuthStrategy], kind: AuthKind) -> bool {
    match kind {
        AuthKind::ApiKey => strategies.contains(&AuthStrategy::ApiKey),
        AuthKind::Jwt => strategies.contains(&AuthStrategy::Jwt),
    }
}
