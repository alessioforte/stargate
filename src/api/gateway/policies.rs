use super::limits::apply_limits;
use super::types::AuthKind;
use crate::err::{ErrorResponse, HttpError};
use crate::etc::{ac::access_control, reqctx, sub::Subject};
use ::http::{HeaderMap, Request};
use axum::body::Body;
use gate::{
    Gate,
    cfg::{AuthStrategy, EnvProfile},
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
    let mut rate_limit_override = None;
    let mut quota_override = None;

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
                    return Err(ErrorResponse::from(HttpError::Unauthorized(
                        "Unauthorized".to_string(),
                    )));
                };
                if !auth_strategy_allowed(strategies, kind) {
                    return Err(ErrorResponse::from(HttpError::Unauthorized(
                        "Unauthorized".to_string(),
                    )));
                }
            }
            PolicyNode::AccessControl { resource, env } => {
                let Some(subject) = subject else {
                    return Err(ErrorResponse::from(HttpError::Unauthorized(
                        "Unauthorized".to_string(),
                    )));
                };

                let profile = env.unwrap_or(EnvProfile::Geo);
                let pe = gate.policy_engine.load();
                let env = reqctx::build_env_from(req.extensions_mut(), profile);
                let allowed = access_control(&pe, subject, &env, resource);

                if !allowed {
                    return Err(ErrorResponse::from(HttpError::Forbidden(
                        "Forbidden".to_string(),
                    )));
                }
            }
            PolicyNode::RateLimit { limit } => {
                if rate_limit_override.replace(limit.clone()).is_some() {
                    return Err(ErrorResponse::from(HttpError::InternalServerError(
                        "Multiple rate_limit policies are not supported".to_string(),
                    )));
                }
            }
            PolicyNode::Quota { limit, cost } => {
                if quota_override.replace((limit.clone(), *cost)).is_some() {
                    return Err(ErrorResponse::from(HttpError::InternalServerError(
                        "Multiple quota policies are not supported".to_string(),
                    )));
                }
            }
        }
    }

    apply_limits(
        gate,
        subject,
        client_ip,
        response_headers,
        rate_limit_override,
        quota_override,
    )
    .await
}

fn auth_strategy_allowed(strategies: &[AuthStrategy], kind: AuthKind) -> bool {
    match kind {
        AuthKind::ApiKey => strategies.contains(&AuthStrategy::ApiKey),
        AuthKind::Jwt => strategies.contains(&AuthStrategy::Jwt),
    }
}
