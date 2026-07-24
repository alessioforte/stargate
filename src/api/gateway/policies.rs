use super::limits::{OrgScopedLimit, SelectedLimitPolicies, apply_limits};
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::{ac::access_control, guard::AuthKind, reqctx, sub::Subject, telemetry};
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
    auth_kind: AuthKind,
    subject: Option<&Subject>,
    client_ip: &str,
    response_headers: &mut HeaderMap,
) -> Result<(), ErrorResponse> {
    let mut selected_limits = SelectedLimitPolicies {
        quota_cost: router.quota_cost,
        ..Default::default()
    };

    for policy_name in &router.policies {
        let policy = graph.policies.get(policy_name).ok_or_else(|| {
            ErrorResponse::new(ErrorCode::GatewayPolicyNotFound)
                .with_param("policy", policy_name.clone())
        })?;

        match policy {
            PolicyNode::Auth { strategies } => {
                if auth_kind == AuthKind::Anonymous {
                    telemetry::record_gateway_policy("auth", "denied");
                    return Err(ErrorResponse::new(ErrorCode::GatewayUnauthorized));
                }
                if !auth_strategy_allowed(strategies, auth_kind) {
                    telemetry::record_gateway_policy("auth", "denied");
                    return Err(ErrorResponse::new(ErrorCode::GatewayUnauthorized));
                }
                telemetry::record_gateway_policy("auth", "allowed");
            }
            PolicyNode::AccessControl { resource, env } => {
                let Some(subject) = subject else {
                    telemetry::record_gateway_policy("access_control", "denied");
                    return Err(ErrorResponse::new(ErrorCode::GatewayUnauthorized));
                };

                let profile = env.unwrap_or(EnvProfile::Geo);
                let pe = gate.policy_engine.load();
                let env = reqctx::build_env_from(req.extensions_mut(), profile);
                let allowed = access_control(&pe, subject, &env, resource);

                if !allowed {
                    telemetry::record_gateway_policy("access_control", "denied");
                    return Err(ErrorResponse::new(ErrorCode::GatewayAccessDenied));
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
                    return Err(
                        ErrorResponse::new(ErrorCode::GatewayPolicyConfigurationInvalid)
                            .with_param("policyType", "rate_limit"),
                    );
                }
            }
            PolicyNode::Quota {
                limit,
                scope,
                on_missing,
            } => {
                let duplicate = match scope {
                    LimitScope::Subject => selected_limits
                        .subject_quota
                        .replace(limit.clone())
                        .is_some(),
                    LimitScope::Org => selected_limits
                        .org_quota
                        .replace(OrgScopedLimit {
                            limit: limit.clone(),
                            on_missing: *on_missing,
                        })
                        .is_some(),
                };
                if duplicate {
                    return Err(
                        ErrorResponse::new(ErrorCode::GatewayPolicyConfigurationInvalid)
                            .with_param("policyType", "quota"),
                    );
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
        AuthKind::Anonymous => false,
    }
}
