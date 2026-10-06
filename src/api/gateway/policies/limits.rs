use crate::act::orgs::OrgLimitOverrides;
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::gate::RuntimeSnapshot;
use crate::etc::http::headers::retry_after_header_value;
use crate::etc::limits::{attribute_name, configuration_error, validate_limit};
use crate::etc::{auth::subject::Subject, observability::telemetry};
use ::http::{HeaderMap, HeaderName, HeaderValue};
use gate::cfg::OnMissingOrg;
use lim::{LimitKind, Limiter, RateLimitDecision};

const DEFAULT_RATE_LIMIT: &str = "default";

/// Limit policies selected by a route: at most one rate limit and one quota
/// per scope. Org-scoped checks run against the subject's active org bucket
/// alongside (before) the subject-scoped ones. `quota_cost` comes from the
/// router — one request costs the same no matter whose bucket it charges.
#[derive(Debug)]
pub struct SelectedLimitPolicies {
    pub subject_rate: Option<String>,
    pub subject_quota: Option<String>,
    pub org_rate: Option<OrgScopedLimit>,
    pub org_quota: Option<OrgScopedLimit>,
    pub quota_cost: u64,
}

impl Default for SelectedLimitPolicies {
    fn default() -> Self {
        Self {
            subject_rate: None,
            subject_quota: None,
            org_rate: None,
            org_quota: None,
            quota_cost: 1,
        }
    }
}

#[derive(Debug)]
pub struct OrgScopedLimit {
    pub limit: String,
    pub on_missing: OnMissingOrg,
}

struct SelectedLimits {
    subject_key: String,
    rate_limit_name: String,
    quota_name: Option<String>,
}

/// Which bucket an org-scoped check should consume when the subject has —
/// or lacks — an org context.
enum OrgTarget<'a> {
    Org(&'a str),
    Ip,
}

pub async fn apply_limits(
    runtime: &RuntimeSnapshot,
    subject: Option<&Subject>,
    client_ip: &str,
    headers: &mut HeaderMap,
    policies: SelectedLimitPolicies,
) -> Result<(), ErrorResponse> {
    let org_id = subject.and_then(|subject| subject.org_id.as_deref());

    // One cached lookup per request, only when a route actually carries
    // org-scoped policies and the subject acts in an org.
    let org_overrides = match org_id {
        Some(org_id) if policies.org_rate.is_some() || policies.org_quota.is_some() => {
            crate::act::orgs::limit_overrides(org_id).await?
        }
        _ => Default::default(),
    };

    apply_selected_limits(
        runtime,
        subject,
        client_ip,
        headers,
        policies,
        &org_overrides,
    )
    .await
}

struct LimitCheck {
    name: String,
    key: String,
    kind: LimitKind,
    scope: &'static str,
}

fn org_check(
    org_id: Option<&str>,
    client_ip: &str,
    policy: Option<&OrgScopedLimit>,
    override_name: Option<&str>,
    kind: LimitKind,
) -> Result<Option<LimitCheck>, ErrorResponse> {
    let Some(policy) = policy else {
        return Ok(None);
    };
    let (prefix, policy_kind) = match kind {
        LimitKind::Rate => ("lim", "rate_limit"),
        LimitKind::Quota => ("quota", "quota"),
    };
    let (name, key, scope) = match org_target(org_id, policy.on_missing, policy_kind)? {
        Some(OrgTarget::Org(id)) => (
            override_name.unwrap_or(&policy.limit),
            format!("{prefix}:org:{id}"),
            "org",
        ),
        Some(OrgTarget::Ip) => (
            policy.limit.as_str(),
            format!("{prefix}:orgip:{client_ip}"),
            "ip",
        ),
        None => return Ok(None),
    };
    Ok(Some(LimitCheck {
        name: name.into(),
        key,
        kind,
        scope,
    }))
}

async fn apply_selected_limits(
    runtime: &RuntimeSnapshot,
    subject: Option<&Subject>,
    client_ip: &str,
    headers: &mut HeaderMap,
    policies: SelectedLimitPolicies,
    overrides: &OrgLimitOverrides,
) -> Result<(), ErrorResponse> {
    let selected = select_limits(
        subject,
        client_ip,
        policies.subject_rate.as_deref(),
        policies.subject_quota.as_deref(),
    )?;
    let org_id = subject.and_then(|subject| subject.org_id.as_deref());
    let limiter = &runtime.core.limiter;
    let mut checks = Vec::with_capacity(4);
    if let Some(check) = org_check(
        org_id,
        client_ip,
        policies.org_rate.as_ref(),
        overrides.rate_limit.as_deref(),
        LimitKind::Rate,
    )? {
        checks.push(check);
    }
    checks.push(LimitCheck {
        name: selected.rate_limit_name,
        key: format!("lim:{}", selected.subject_key),
        kind: LimitKind::Rate,
        scope: "subject",
    });
    if let Some(check) = org_check(
        org_id,
        client_ip,
        policies.org_quota.as_ref(),
        overrides.quota.as_deref(),
        LimitKind::Quota,
    )? {
        checks.push(check);
    }
    if let Some(name) = selected.quota_name {
        checks.push(LimitCheck {
            name,
            key: format!("quota:{}", selected.subject_key),
            kind: LimitKind::Quota,
            scope: "subject",
        });
    }
    // All selected references must be valid before any consuming check runs.
    // Config/attribute mistakes must not charge unrelated valid buckets.
    for check in &checks {
        validate_limit(limiter, &check.name, check.kind, check.scope)?;
    }
    let mut rate_checks = Vec::with_capacity(2);
    let mut quota_checks = Vec::with_capacity(2);
    for check in checks {
        let cost = (check.kind == LimitKind::Quota).then_some(policies.quota_cost);
        let decision = run_check(limiter, &check, cost).await?;
        match check.kind {
            LimitKind::Rate => push_rate_check(&mut rate_checks, decision, check.scope)?,
            LimitKind::Quota => push_quota_check(&mut quota_checks, decision, check.scope)?,
        }
    }
    if let Some((decision, scope)) = binding_check(&rate_checks) {
        insert_limit_headers(headers, decision, scope, "x-ratelimit");
    }
    if let Some((decision, scope)) = binding_check(&quota_checks) {
        insert_limit_headers(headers, decision, scope, "x-quota");
    }
    Ok(())
}

fn org_target<'a>(
    org_id: Option<&'a str>,
    on_missing: OnMissingOrg,
    policy_kind: &'static str,
) -> Result<Option<OrgTarget<'a>>, ErrorResponse> {
    match org_id {
        Some(org_id) => Ok(Some(OrgTarget::Org(org_id))),
        None => match on_missing {
            OnMissingOrg::Skip => Ok(None),
            OnMissingOrg::IpFallback => Ok(Some(OrgTarget::Ip)),
            OnMissingOrg::Deny => {
                telemetry::record_gateway_policy(policy_kind, "denied");
                Err(ErrorResponse::new(
                    ErrorCode::GatewayOrganizationContextRequired,
                ))
            }
        },
    }
}

async fn run_check(
    limiter: &Limiter,
    check: &LimitCheck,
    cost: Option<u64>,
) -> Result<RateLimitDecision, ErrorResponse> {
    limiter
        .check(&check.name, &check.key, cost)
        .await
        .map_err(|error| {
            if matches!(
                error,
                lim::RateLimitError::UnknownLimit(_)
                    | lim::RateLimitError::IncompatibleLimit { .. }
            ) {
                return configuration_error(&check.name, check.kind, check.scope, &error);
            }
            telemetry::record_gateway_policy(&check.kind.to_string(), "error");
            tracing::error!(policy_type = %check.kind, "Limiter error: {error}");
            ErrorResponse::new(ErrorCode::GatewayLimiterUnavailable)
        })
}

fn push_rate_check(
    checks: &mut Vec<(RateLimitDecision, &'static str)>,
    decision: RateLimitDecision,
    scope: &'static str,
) -> Result<(), ErrorResponse> {
    if !decision.is_allowed() {
        telemetry::record_gateway_policy("rate_limit", "denied");
        return Err(limited_response(
            &decision,
            scope,
            ErrorCode::GatewayRateLimitExceeded,
            "x-ratelimit",
        ));
    }
    telemetry::record_gateway_policy("rate_limit", "allowed");
    checks.push((decision, scope));
    Ok(())
}

fn push_quota_check(
    checks: &mut Vec<(RateLimitDecision, &'static str)>,
    decision: RateLimitDecision,
    scope: &'static str,
) -> Result<(), ErrorResponse> {
    if !decision.is_allowed() {
        telemetry::record_gateway_policy("quota", "denied");
        return Err(limited_response(
            &decision,
            scope,
            ErrorCode::GatewayQuotaExceeded,
            "x-quota",
        ));
    }
    telemetry::record_gateway_policy("quota", "allowed");
    checks.push((decision, scope));
    Ok(())
}

fn limited_response(
    decision: &RateLimitDecision,
    scope: &'static str,
    code: ErrorCode,
    header_prefix: &str,
) -> ErrorResponse {
    let retry_after = retry_after_header_value(decision.retry_after);
    let mut response = ErrorResponse::new(code);
    response
        .insert_header("retry-after", &retry_after)
        .insert_header(
            &format!("{header_prefix}-limit"),
            &decision.limit.to_string(),
        )
        .insert_header(
            &format!("{header_prefix}-remaining"),
            &decision.remaining.to_string(),
        )
        .insert_header(&format!("{header_prefix}-scope"), scope);
    response
}

/// The check to report in response headers: the one closest to exhaustion.
fn binding_check<'a>(
    checks: &'a [(RateLimitDecision, &'static str)],
) -> Option<(&'a RateLimitDecision, &'static str)> {
    checks
        .iter()
        .min_by_key(|(decision, _)| decision.remaining)
        .map(|(decision, scope)| (decision, *scope))
}

fn insert_limit_headers(
    headers: &mut HeaderMap,
    decision: &RateLimitDecision,
    scope: &'static str,
    prefix: &str,
) {
    let entries = [
        (format!("{prefix}-limit"), decision.limit.to_string()),
        (
            format!("{prefix}-remaining"),
            decision.remaining.to_string(),
        ),
        (format!("{prefix}-scope"), scope.to_string()),
    ];
    for (name, value) in entries {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            headers.insert(name, value);
        }
    }
}

fn select_limits(
    subject: Option<&Subject>,
    client_ip: &str,
    rate_limit_override: Option<&str>,
    quota_override: Option<&str>,
) -> Result<SelectedLimits, ErrorResponse> {
    let mut rate_limit_name = rate_limit_override
        .unwrap_or(DEFAULT_RATE_LIMIT)
        .to_string();
    let mut quota_name = quota_override.map(str::to_string);
    let mut subject_key = client_ip.to_string();

    if let Some(subject) = subject {
        if rate_limit_name == DEFAULT_RATE_LIMIT
            && let Some(rate_limit) =
                attribute_name(&subject.attrs, "rate_limit", LimitKind::Rate, "subject")?
        {
            rate_limit_name = rate_limit.to_string();
        }

        if quota_name.is_none()
            && let Some(quota) =
                attribute_name(&subject.attrs, "quota", LimitKind::Quota, "subject")?
        {
            quota_name = Some(quota.to_string());
        }

        subject_key = subject.id.clone();
    }

    Ok(SelectedLimits {
        subject_key,
        rate_limit_name,
        quota_name,
    })
}

#[cfg(test)]
mod tests;

#[cfg(all(test, feature = "memory"))]
mod validation_tests;
