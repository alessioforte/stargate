use super::*;
use crate::etc::{
    auth::subject::SubjectType,
    gate::{prepare_config, test_support},
};
use gate::cfg::Config;
use serde_json::{Value, json};
use std::sync::Arc;

const IP: &str = "203.0.113.8";
const SUBJECT: &str = "subject";
const ORG: &str = "organization";

fn config() -> Config {
    let mut value = serde_json::to_value(Config::default()).unwrap();
    for (name, burst) in [("default", 1), ("org", 1), ("premium", 2)] {
        value["limits"][name] =
            json!({"strategy":"gcra","params":{"max_burst":burst,"replenish_1_per":"1h"}});
    }
    for (name, limit) in [("daily", 3), ("org-daily", 4)] {
        value["limits"][name] =
            json!({"strategy":"quota_tracker","params":{"limit":limit,"period":"day"}});
    }
    value["http"] = json!({
        "services":{"local":{"kind":"direct_response","status":200}},
        "routers":{"all":{"match":{"path":{"prefix":"/"}},"service":"local"}}
    });
    serde_json::from_value(value).unwrap()
}

fn subject(attrs: Value, organization: bool) -> Subject {
    let mut subject = Subject::new(SUBJECT.into(), SubjectType::User, Some(attrs));
    subject.org_id = organization.then(|| ORG.into());
    subject
}

fn org_policies(on_missing: OnMissingOrg) -> SelectedLimitPolicies {
    SelectedLimitPolicies {
        org_rate: Some(OrgScopedLimit {
            limit: "org".into(),
            on_missing,
        }),
        org_quota: Some(OrgScopedLimit {
            limit: "org-daily".into(),
            on_missing,
        }),
        ..Default::default()
    }
}

async fn assert_buckets_uncharged(runtime: &RuntimeSnapshot) {
    for (name, key, cost) in [
        ("default", format!("lim:{SUBJECT}"), 1),
        ("org", format!("lim:org:{ORG}"), 1),
        ("daily", format!("quota:{SUBJECT}"), 3),
        ("org-daily", format!("quota:org:{ORG}"), 4),
    ] {
        let decision = runtime
            .core
            .limiter
            .check(name, &key, Some(cost))
            .await
            .unwrap();
        assert!(decision.is_allowed(), "invalid selection charged {name}");
        assert_eq!(decision.remaining, 0);
    }
}

#[tokio::test]
async fn invalid_subject_and_org_names_and_strategies_fail_before_any_resource_bucket_is_charged() {
    for (scope, kind, name) in [
        ("subject", LimitKind::Rate, "typo"),
        ("subject", LimitKind::Quota, "typo"),
        ("org", LimitKind::Rate, "typo"),
        ("org", LimitKind::Quota, "typo"),
        ("subject", LimitKind::Rate, "daily"),
        ("subject", LimitKind::Quota, "default"),
        ("org", LimitKind::Rate, "org-daily"),
        ("org", LimitKind::Quota, "org"),
    ] {
        let gate = test_support::gate(config());
        let runtime = gate.snapshot();
        let mut subject = subject(json!({"quota":"daily"}), true);
        let mut overrides = OrgLimitOverrides {
            rate_limit: Some("org".into()),
            quota: Some("org-daily".into()),
        };
        match (scope, kind) {
            ("subject", LimitKind::Rate) => subject.attrs["rate_limit"] = json!(name),
            ("subject", LimitKind::Quota) => subject.attrs["quota"] = json!(name),
            ("org", LimitKind::Rate) => overrides.rate_limit = Some(name.into()),
            ("org", LimitKind::Quota) => overrides.quota = Some(name.into()),
            _ => unreachable!(),
        }
        let mut headers = HeaderMap::new();
        let error = apply_selected_limits(
            &runtime,
            Some(&subject),
            IP,
            &mut headers,
            org_policies(OnMissingOrg::Deny),
            &overrides,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::GatewayLimitConfigurationInvalid);
        assert_eq!(error.status, http::StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.params["scope"], scope);
        assert_eq!(error.params["policyType"], kind.to_string());
        assert_eq!(
            error.params["reason"],
            if name == "typo" {
                "unknown_limit"
            } else {
                "incompatible_strategy"
            }
        );
        assert!(headers.is_empty());
        assert_buckets_uncharged(&runtime).await;
    }
}

#[tokio::test]
async fn route_precedence_ignores_unselected_subject_overrides_and_known_limits_stay_independent() {
    let gate = test_support::gate(config());
    let runtime = gate.snapshot();
    let subject = subject(json!({"rate_limit":"typo","quota":"typo"}), false);
    let mut headers = HeaderMap::new();
    apply_limits(
        &runtime,
        Some(&subject),
        IP,
        &mut headers,
        SelectedLimitPolicies {
            subject_rate: Some("premium".into()),
            subject_quota: Some("daily".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(headers["x-ratelimit-limit"], "1");
    assert_eq!(headers["x-quota-limit"], "3");
    assert_eq!(headers["x-quota-remaining"], "2");
    assert!(
        runtime
            .core
            .limiter
            .check("default", "lim:subject", None)
            .await
            .unwrap()
            .is_allowed()
    );
    assert!(
        runtime
            .core
            .limiter
            .check("premium", "lim:subject", None)
            .await
            .unwrap()
            .is_allowed()
    );
    assert!(
        !runtime
            .core
            .limiter
            .check("premium", "lim:subject", None)
            .await
            .unwrap()
            .is_allowed()
    );
}

#[tokio::test]
async fn absent_quotas_and_explicit_org_skip_do_not_select_or_consume_a_bucket() {
    let gate = test_support::gate(config());
    let runtime = gate.snapshot();
    let subject = subject(json!({"quota":null,"rate_limit":null}), false);
    let overrides = OrgLimitOverrides {
        rate_limit: Some("typo".into()),
        quota: Some("typo".into()),
    };
    let mut headers = HeaderMap::new();
    apply_selected_limits(
        &runtime,
        Some(&subject),
        IP,
        &mut headers,
        org_policies(OnMissingOrg::Skip),
        &overrides,
    )
    .await
    .unwrap();
    assert_eq!(headers["x-ratelimit-limit"], "1");
    assert!(!headers.contains_key("x-quota-limit"));
    for (name, key, cost) in [
        ("org", "lim:org:organization", 1),
        ("daily", "quota:subject", 3),
        ("org-daily", "quota:org:organization", 4),
    ] {
        assert!(
            runtime
                .core
                .limiter
                .check(name, key, Some(cost))
                .await
                .unwrap()
                .is_allowed()
        );
    }
}

#[tokio::test]
async fn org_ip_fallback_uses_the_policy_names_and_original_namespaces() {
    let gate = test_support::gate(config());
    let runtime = gate.snapshot();
    let overrides = OrgLimitOverrides {
        rate_limit: Some("typo".into()),
        quota: Some("typo".into()),
    };
    let mut headers = HeaderMap::new();
    apply_selected_limits(
        &runtime,
        None,
        IP,
        &mut headers,
        org_policies(OnMissingOrg::IpFallback),
        &overrides,
    )
    .await
    .unwrap();
    assert_eq!(headers["x-quota-scope"], "ip");
    assert!(
        !runtime
            .core
            .limiter
            .check("org", &format!("lim:orgip:{IP}"), None)
            .await
            .unwrap()
            .is_allowed()
    );
    assert_eq!(
        runtime
            .core
            .limiter
            .check("org-daily", &format!("quota:orgip:{IP}"), Some(3))
            .await
            .unwrap()
            .remaining,
        0
    );
    assert!(
        runtime
            .core
            .limiter
            .check("org", "lim:org:organization", None)
            .await
            .unwrap()
            .is_allowed()
    );
}

#[tokio::test]
async fn malformed_selected_attributes_are_errors_and_diagnostics_are_bounded() {
    for field in ["rate_limit", "quota"] {
        for value in [json!(5), json!(false), json!({}), json!([])] {
            let gate = test_support::gate(config());
            let runtime = gate.snapshot();
            let subject = subject(json!({field:value}), false);
            let error = apply_limits(
                &runtime,
                Some(&subject),
                IP,
                &mut HeaderMap::new(),
                SelectedLimitPolicies::default(),
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, ErrorCode::GatewayLimitConfigurationInvalid);
            assert_eq!(error.params["field"], format!("attrs.{field}"));
            assert!(
                runtime
                    .core
                    .limiter
                    .check("default", "lim:subject", None)
                    .await
                    .unwrap()
                    .is_allowed()
            );
        }
    }
    let gate = test_support::gate(config());
    let runtime = gate.snapshot();
    let subject = subject(json!({"rate_limit":"é".repeat(1000)}), false);
    let error = apply_limits(
        &runtime,
        Some(&subject),
        IP,
        &mut HeaderMap::new(),
        SelectedLimitPolicies::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.params["limit"].as_str().unwrap().chars().count(), 64);
}

#[tokio::test]
async fn removing_an_attribute_referenced_limit_fails_new_requests_but_preserves_the_in_flight_snapshot()
 {
    use crate::api::gateway::{IngressPause, authentication::TestAuthenticator};
    use crate::etc::auth::identity::VerifiedIdentity;
    use axum::{body::Body, extract::ConnectInfo};
    use http::Request;
    use http_body_util::BodyExt;
    use tokio::sync::Barrier;

    let gate = Arc::new(test_support::gate(config()));
    let pause = IngressPause {
        pinned: Arc::new(Barrier::new(2)),
        resume: Arc::new(Barrier::new(2)),
    };
    let identity = VerifiedIdentity::test_jwt(
        subject(json!({"rate_limit":"premium"}), false),
        "session".into(),
        Some(1_784_473_000),
    );
    let authenticator = TestAuthenticator {
        verify: Arc::new(move |_| identity.clone()),
    };
    let request = || {
        let mut req = Request::new(Body::empty());
        req.extensions_mut().insert(gate.clone());
        req.extensions_mut().insert(authenticator.clone());
        req.extensions_mut().insert(ConnectInfo(
            "203.0.113.8:443".parse::<std::net::SocketAddr>().unwrap(),
        ));
        req
    };
    let mut old_request = request();
    old_request.extensions_mut().insert(pause.clone());
    let task = tokio::spawn(crate::api::gateway::service(old_request));
    pause.pinned.wait().await;
    let mut removed = config();
    removed.limits.shift_remove("premium");
    gate.activate(prepare_config(removed).unwrap()).unwrap();
    pause.resume.wait().await;
    let old_response = task.await.unwrap().unwrap();
    assert_eq!(old_response.status(), 200);
    assert_eq!(old_response.headers()["x-ratelimit-limit"], "1");
    old_response.into_body().collect().await.unwrap();
    let response = crate::api::gateway::service(request()).await.unwrap();
    assert_eq!(response.status(), 500);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["code"],
        "gateway.limit_configuration_invalid"
    );
    assert!(
        gate.snapshot()
            .core
            .limiter
            .check("default", "lim:subject", None)
            .await
            .unwrap()
            .is_allowed()
    );
}
