use super::limits::{OrgScopedLimit, SelectedLimitPolicies, apply_limits};
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::{
    ac::access_control,
    guard::{AuthKind, VerifiedIdentity},
    reqctx, telemetry,
};
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
    policy_engine: &ace::PolicyEngine,
    policy_revision: &str,
    req: &mut Request<Body>,
    identity: &VerifiedIdentity,
    client_ip: &str,
    response_headers: &mut HeaderMap,
) -> Result<(), ErrorResponse> {
    let subject = identity.subject();
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
            PolicyNode::Auth {
                strategies,
                audience,
            } => {
                if !auth_policy_allows(strategies, audience.as_deref(), identity) {
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
                let env = reqctx::build_env_from(req.extensions_mut(), profile);
                let allowed =
                    access_control(policy_engine, policy_revision, subject, &env, resource);

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
        AuthKind::OAuth => strategies.contains(&AuthStrategy::OAuth),
        AuthKind::Anonymous => false,
    }
}

fn auth_policy_allows(
    strategies: &[AuthStrategy],
    audience: Option<&str>,
    identity: &VerifiedIdentity,
) -> bool {
    if !auth_strategy_allowed(strategies, identity.auth_kind()) {
        return false;
    }
    if identity.auth_kind() != AuthKind::OAuth {
        return true;
    }
    audience.is_some() && identity.oauth_audience() == audience
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etc::{
        guard::VerifiedIdentity,
        sub::{Subject, SubjectType},
    };

    fn user() -> Subject {
        Subject::new(
            "01JZ000000000000000000000A".to_owned(),
            SubjectType::User,
            None,
        )
    }

    #[test]
    fn oauth_auth_requires_strategy_and_exact_audience() {
        let identity = VerifiedIdentity::test_oauth(
            user(),
            "01JZ000000000000000000000B".to_owned(),
            1_784_473_000,
            Some("gateway".to_owned()),
        );

        assert!(auth_policy_allows(
            &[AuthStrategy::OAuth],
            Some("gateway"),
            &identity
        ));
        assert!(!auth_policy_allows(
            &[AuthStrategy::OAuth],
            Some("other-api"),
            &identity
        ));
        assert!(!auth_policy_allows(
            &[AuthStrategy::Jwt],
            Some("gateway"),
            &identity
        ));
    }

    #[test]
    fn native_jwt_is_not_subject_to_oauth_audience() {
        let identity = VerifiedIdentity::test_jwt(
            user(),
            "01JZ000000000000000000000B".to_owned(),
            Some(1_784_473_000),
        );

        assert!(auth_policy_allows(
            &[AuthStrategy::Jwt, AuthStrategy::OAuth],
            Some("gateway"),
            &identity
        ));
    }
}
