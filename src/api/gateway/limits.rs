use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::gate::RuntimeSnapshot;
use crate::etc::{sub::Subject, telemetry};
use ::http::{HeaderMap, HeaderName, HeaderValue};
use gate::cfg::OnMissingOrg;
use lim::{Limiter, RateLimitDecision};

const DEFAULT_RATE_LIMIT: &str = "default";

pub(super) async fn apply_ingress_limit(
    runtime: &RuntimeSnapshot,
    client_ip: &str,
    execution: &super::lifecycle::Execution,
) -> Result<(), ErrorResponse> {
    let limiter = &runtime.core.limiter;
    let ingress = &runtime.ingress;
    if !limiter.has_limit(&ingress.limit) {
        telemetry::record_gateway_policy("ingress", "error");
        return Err(ErrorResponse::new(ErrorCode::GatewayIngressUnavailable));
    }
    // This namespace never intersects subject/org rate or quota buckets, even
    // when the ingress and resource policies reference the same named limit.
    let key = format!("ingress:ip:{client_ip}");
    let decision = match execution
        .run(
            "ingress",
            ingress.timeout,
            limiter.check(&ingress.limit, &key, None),
        )
        .await
    {
        Ok(Ok(decision)) => decision,
        Ok(Err(error)) => {
            telemetry::record_gateway_policy("ingress", "error");
            tracing::error!(%error, "Ingress limiter failed");
            return Err(ErrorResponse::new(ErrorCode::GatewayIngressUnavailable));
        }
        Err(error)
            if error.code == ErrorCode::GatewayTimeout
                && error
                    .params
                    .get("phase")
                    .is_some_and(|phase| phase == "ingress") =>
        {
            telemetry::record_gateway_policy("ingress", "timeout");
            return Err(ErrorResponse::new(ErrorCode::GatewayIngressUnavailable)
                .with_param("phase", "ingress"));
        }
        Err(error) => return Err(error),
    };
    if decision.is_allowed() {
        telemetry::record_gateway_policy("ingress", "allowed");
        return Ok(());
    }
    telemetry::record_gateway_policy("ingress", "denied");
    telemetry::record_gateway_rejection("ingress");
    let retry = decision
        .retry_after
        .unwrap_or(std::time::Duration::from_secs(60));
    // RFC 9110 section 10.2.3: delay-seconds is a nonnegative decimal integer.
    let retry_seconds = retry
        .as_secs()
        .saturating_add(u64::from(retry.subsec_nanos() > 0))
        .max(1);
    let mut error = ErrorResponse::new(ErrorCode::GatewayIngressRateLimitExceeded);
    error
        .insert_header("Retry-After", &retry_seconds.to_string())
        .insert_header("X-RateLimit-Limit", &decision.limit.to_string())
        .insert_header("X-RateLimit-Remaining", &decision.remaining.to_string())
        .insert_header("X-RateLimit-Scope", "ingress");
    Err(error)
}

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
    let limiter = &runtime.core.limiter;
    let org_id = subject.and_then(|subject| subject.org_id.as_deref());

    // One cached lookup per request, only when a route actually carries
    // org-scoped policies and the subject acts in an org.
    let org_overrides = match org_id {
        Some(org_id) if policies.org_rate.is_some() || policies.org_quota.is_some() => {
            crate::act::orgs::limit_overrides(org_id).await
        }
        _ => Default::default(),
    };

    let selected = select_limits(
        subject,
        client_ip,
        policies.subject_rate.as_deref(),
        policies.subject_quota.as_deref(),
    );

    // ── Rate limits: org bucket first (the coarser gate), then subject ──────
    let mut rate_checks: Vec<(RateLimitDecision, &'static str)> = Vec::with_capacity(2);

    if let Some(org_rate) = &policies.org_rate {
        match org_target(org_id, org_rate.on_missing, "rate_limit")? {
            Some(OrgTarget::Org(org_id)) => {
                let name = org_overrides
                    .rate_limit
                    .as_deref()
                    .unwrap_or(&org_rate.limit);
                let key = format!("lim:org:{org_id}");
                tracing::debug!(name, key, "org rate check");
                let decision = run_check(&limiter, name, &key, None, "rate_limit").await?;
                push_rate_check(&mut rate_checks, decision, "org")?;
            }
            Some(OrgTarget::Ip) => {
                let key = format!("lim:orgip:{client_ip}");
                let decision =
                    run_check(&limiter, &org_rate.limit, &key, None, "rate_limit").await?;
                push_rate_check(&mut rate_checks, decision, "ip")?;
            }
            None => {}
        }
    }

    {
        let mut key = String::with_capacity(4 + selected.subject_key.len());
        key.push_str("lim:");
        key.push_str(&selected.subject_key);
        let decision = run_check(
            &limiter,
            &selected.rate_limit_name,
            &key,
            None,
            "rate_limit",
        )
        .await?;
        push_rate_check(&mut rate_checks, decision, "subject")?;
    }

    if let Some((decision, scope)) = binding_check(&rate_checks) {
        insert_limit_headers(headers, decision, scope, "x-ratelimit");
    }

    // ── Quotas: same scope order. A denial after an earlier consuming check
    // leaves that earlier bucket charged for a rejected request — accepted
    // imprecision with consume-on-check strategies. ──────────────────────────
    let mut quota_checks: Vec<(RateLimitDecision, &'static str)> = Vec::with_capacity(2);

    if let Some(org_quota) = &policies.org_quota {
        match org_target(org_id, org_quota.on_missing, "quota")? {
            Some(OrgTarget::Org(org_id)) => {
                let name = org_overrides.quota.as_deref().unwrap_or(&org_quota.limit);
                let key = format!("quota:org:{org_id}");
                let decision =
                    run_check(&limiter, name, &key, Some(policies.quota_cost), "quota").await?;
                push_quota_check(&mut quota_checks, decision, "org")?;
            }
            Some(OrgTarget::Ip) => {
                let key = format!("quota:orgip:{client_ip}");
                let decision = run_check(
                    &limiter,
                    &org_quota.limit,
                    &key,
                    Some(policies.quota_cost),
                    "quota",
                )
                .await?;
                push_quota_check(&mut quota_checks, decision, "ip")?;
            }
            None => {}
        }
    }

    if let Some(quota_name) = &selected.quota_name {
        let mut key = String::with_capacity(6 + selected.subject_key.len());
        key.push_str("quota:");
        key.push_str(&selected.subject_key);
        let decision = run_check(
            &limiter,
            quota_name,
            &key,
            Some(policies.quota_cost),
            "quota",
        )
        .await?;
        push_quota_check(&mut quota_checks, decision, "subject")?;
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
    name: &str,
    key: &str,
    cost: Option<u64>,
    policy_kind: &'static str,
) -> Result<RateLimitDecision, ErrorResponse> {
    limiter.check(name, key, cost).await.map_err(|error| {
        telemetry::record_gateway_policy(policy_kind, "error");
        tracing::error!("{policy_kind} limiter error: {error}");
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
    let retry_after = decision
        .retry_after
        .unwrap_or(std::time::Duration::from_secs(60));
    let retry_after = retry_after_header_value(retry_after);
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
/// Checks against an unconfigured limit name report `limit == 0` and are
/// ignored unless they are all there is.
fn binding_check<'a>(
    checks: &'a [(RateLimitDecision, &'static str)],
) -> Option<(&'a RateLimitDecision, &'static str)> {
    checks
        .iter()
        .filter(|(decision, _)| decision.limit > 0)
        .min_by_key(|(decision, _)| decision.remaining)
        .or_else(|| checks.last())
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
) -> SelectedLimits {
    let mut rate_limit_name = rate_limit_override
        .unwrap_or(DEFAULT_RATE_LIMIT)
        .to_string();
    let mut quota_name = quota_override.map(str::to_string);
    let mut subject_key = client_ip.to_string();

    if let Some(subject) = subject {
        if rate_limit_name == DEFAULT_RATE_LIMIT
            && let Some(rate_limit) = subject
                .get_attr("rate_limit")
                .and_then(|value| value.as_str())
        {
            rate_limit_name = rate_limit.to_string();
        }

        if quota_name.is_none()
            && let Some(quota) = subject.get_attr("quota").and_then(|value| value.as_str())
        {
            quota_name = Some(quota.to_string());
        }

        subject_key = subject.id.clone();
    }

    SelectedLimits {
        subject_key,
        rate_limit_name,
        quota_name,
    }
}

fn retry_after_header_value(retry_after: std::time::Duration) -> String {
    let retry_after = match chrono::Duration::from_std(retry_after) {
        Ok(retry_after) => retry_after,
        Err(error) => {
            tracing::warn!(
                error = ?error,
                "Retry-After duration overflowed chrono::Duration; defaulting to zero"
            );
            chrono::Duration::default()
        }
    };

    tools::duration_to_string(&retry_after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etc::sub::{Subject, SubjectType};
    use serde_json::json;

    fn subject(attrs: serde_json::Value) -> Subject {
        Subject {
            id: "sub_123".to_string(),
            sub_type: SubjectType::ApiKey,
            org_id: None,
            org_role: None,
            attrs,
        }
    }

    #[test]
    fn selects_default_rate_limit_for_anonymous_requests() {
        let selected = select_limits(None, "203.0.113.10", None, None);

        assert_eq!(selected.subject_key, "203.0.113.10");
        assert_eq!(selected.rate_limit_name, "default");
        assert_eq!(selected.quota_name, None);
    }

    #[test]
    fn subject_attrs_override_implicit_default_rate_limit() {
        let subject = subject(json!({
            "rate_limit": "premium",
            "quota": "monthly-premium"
        }));

        let selected = select_limits(Some(&subject), "203.0.113.10", None, None);

        assert_eq!(selected.subject_key, "sub_123");
        assert_eq!(selected.rate_limit_name, "premium");
        assert_eq!(selected.quota_name, Some("monthly-premium".to_string()));
    }

    #[test]
    fn subject_attrs_override_explicit_default_rate_limit_policy() {
        let subject = subject(json!({
            "rate_limit": "premium"
        }));

        let selected = select_limits(Some(&subject), "203.0.113.10", Some("default"), None);

        assert_eq!(selected.rate_limit_name, "premium");
    }

    #[test]
    fn resource_rate_limit_policy_overrides_subject_rate_limit_attr() {
        let subject = subject(json!({
            "rate_limit": "premium"
        }));

        let selected = select_limits(Some(&subject), "203.0.113.10", Some("reports"), None);

        assert_eq!(selected.rate_limit_name, "reports");
    }

    #[test]
    fn quota_policy_overrides_subject_quota_attr() {
        let subject = subject(json!({
            "quota": "monthly-premium"
        }));

        let selected = select_limits(Some(&subject), "203.0.113.10", None, Some("daily-reports"));

        assert_eq!(selected.rate_limit_name, "default");
        assert_eq!(selected.quota_name, Some("daily-reports".to_string()));
    }

    #[test]
    fn binding_check_prefers_configured_limit_closest_to_exhaustion() {
        let checks = vec![
            (RateLimitDecision::allowed(100, 90, None), "org"),
            (RateLimitDecision::allowed(10, 2, None), "subject"),
        ];
        let (decision, scope) = binding_check(&checks).unwrap();
        assert_eq!(scope, "subject");
        assert_eq!(decision.remaining, 2);
    }

    #[test]
    fn binding_check_ignores_unconfigured_limits_when_possible() {
        let checks = vec![
            // Unconfigured limiter name: allowed with limit 0.
            (RateLimitDecision::allowed(0, 0, None), "org"),
            (RateLimitDecision::allowed(50, 49, None), "subject"),
        ];
        let (decision, scope) = binding_check(&checks).unwrap();
        assert_eq!(scope, "subject");
        assert_eq!(decision.limit, 50);

        let only_unconfigured = vec![(RateLimitDecision::allowed(0, 0, None), "subject")];
        let (decision, scope) = binding_check(&only_unconfigured).unwrap();
        assert_eq!(scope, "subject");
        assert_eq!(decision.limit, 0);

        assert!(binding_check(&[]).is_none());
    }

    #[test]
    fn org_target_resolution_honors_on_missing() {
        assert!(matches!(
            org_target(Some("org_a"), OnMissingOrg::Deny, "rate_limit"),
            Ok(Some(OrgTarget::Org("org_a")))
        ));
        assert!(matches!(
            org_target(None, OnMissingOrg::Skip, "rate_limit"),
            Ok(None)
        ));
        assert!(matches!(
            org_target(None, OnMissingOrg::IpFallback, "rate_limit"),
            Ok(Some(OrgTarget::Ip))
        ));
        assert!(org_target(None, OnMissingOrg::Deny, "rate_limit").is_err());
    }
}
