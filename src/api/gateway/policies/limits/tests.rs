use super::*;
use crate::etc::auth::subject::{Subject, SubjectType};
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
    let selected = select_limits(None, "203.0.113.10", None, None).unwrap();

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

    let selected = select_limits(Some(&subject), "203.0.113.10", None, None).unwrap();

    assert_eq!(selected.subject_key, "sub_123");
    assert_eq!(selected.rate_limit_name, "premium");
    assert_eq!(selected.quota_name, Some("monthly-premium".to_string()));
}

#[test]
fn subject_attrs_override_explicit_default_rate_limit_policy() {
    let subject = subject(json!({
        "rate_limit": "premium"
    }));

    let selected = select_limits(Some(&subject), "203.0.113.10", Some("default"), None).unwrap();

    assert_eq!(selected.rate_limit_name, "premium");
}

#[test]
fn resource_rate_limit_policy_overrides_subject_rate_limit_attr() {
    let subject = subject(json!({
        "rate_limit": "premium"
    }));

    let selected = select_limits(Some(&subject), "203.0.113.10", Some("reports"), None).unwrap();

    assert_eq!(selected.rate_limit_name, "reports");
}

#[test]
fn quota_policy_overrides_subject_quota_attr() {
    let subject = subject(json!({
        "quota": "monthly-premium"
    }));

    let selected =
        select_limits(Some(&subject), "203.0.113.10", None, Some("daily-reports")).unwrap();

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
