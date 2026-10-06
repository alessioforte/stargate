use super::*;
use crate::etc::auth::subject::SubjectType;
use serde_json::json;

fn engine(policies: &str) -> ace::PolicyEngine {
    let mut engine = ace::PolicyEngine::new();
    engine.parse_file(policies).unwrap();
    engine
}

fn user_subject(org_id: Option<&str>, org_role: Option<&str>) -> Subject {
    let mut subject = Subject::new("user_1".to_string(), SubjectType::User, None);
    subject.org_id = org_id.map(str::to_string);
    subject.org_role = org_role.map(str::to_string);
    subject
}

#[test]
fn org_role_condition_gates_access() {
    let pe = engine(r#"ALLOW user FOR "reports" WHEN user.org_role == "admin";"#);
    let env = Env::default();

    let admin = user_subject(Some("org_a"), Some("admin"));
    let member = user_subject(Some("org_a"), Some("member"));
    let orgless = user_subject(None, None);

    assert!(access_control(&pe, "test", &admin, &env, "reports"));
    assert!(!access_control(&pe, "test", &member, &env, "reports"));
    assert!(!access_control(&pe, "test", &orgless, &env, "reports"));
}

/// Identical attrs, different orgs: the second evaluation must not hit
/// the first one's cached decision.
#[test]
fn decision_cache_does_not_leak_across_orgs() {
    let pe = engine(r#"ALLOW user FOR "billing" WHEN user.org_id == "org_a";"#);
    let env = Env::default();

    let in_org_a = user_subject(Some("org_a"), Some("member"));
    let in_org_b = user_subject(Some("org_b"), Some("member"));

    // Prime the cache with the allowed decision, then flip org.
    assert!(access_control(&pe, "test", &in_org_a, &env, "billing"));
    assert!(!access_control(&pe, "test", &in_org_b, &env, "billing"));
    // And back: org_a's cached decision is still the right one.
    assert!(access_control(&pe, "test", &in_org_a, &env, "billing"));
}

/// The same subject switching org context (same attrs hash) must be
/// re-evaluated, not served the pre-switch decision.
#[test]
fn decision_cache_does_not_survive_org_switch() {
    let pe = engine(r#"ALLOW user FOR "exports" WHEN user.org_role == "owner";"#);
    let env = Env::default();

    let as_owner = user_subject(Some("org_a"), Some("owner"));
    assert!(access_control(&pe, "test", &as_owner, &env, "exports"));

    let as_member = user_subject(Some("org_a"), Some("member"));
    assert!(!access_control(&pe, "test", &as_member, &env, "exports"));
}

#[test]
fn api_key_subjects_expose_org_id() {
    let pe = engine(r#"ALLOW api_key FOR "ingest" WHEN api_key.org_id == "org_a";"#);
    let env = Env::default();

    let mut bound = Subject::new("key_1".to_string(), SubjectType::ApiKey, None);
    bound.org_id = Some("org_a".to_string());
    let unbound = Subject::new("key_2".to_string(), SubjectType::ApiKey, None);

    assert!(access_control(&pe, "test", &bound, &env, "ingest"));
    assert!(!access_control(&pe, "test", &unbound, &env, "ingest"));
}

/// Org context comes from the validated membership; a subject attr with
/// the same name must not shadow it.
#[test]
fn membership_org_wins_over_attrs_of_the_same_name() {
    let pe = engine(r#"ALLOW user FOR "wire" WHEN user.org_id == "org_spoofed";"#);
    let env = Env::default();

    let mut subject = Subject::new(
        "user_1".to_string(),
        SubjectType::User,
        Some(json!({ "org_id": "org_spoofed" })),
    );
    subject.org_id = Some("org_real".to_string());

    assert!(!access_control(&pe, "test", &subject, &env, "wire"));
}

#[test]
fn decision_cache_resets_when_policy_revision_changes() {
    let allowed = engine(r#"ALLOW user FOR "reports";"#);
    let denied = engine(r#"DENY user FOR "reports";"#);
    let subject = user_subject(None, None);
    let env = Env::default();

    assert!(access_control(
        &allowed,
        "sha256:old",
        &subject,
        &env,
        "reports"
    ));
    assert!(!access_control(
        &denied,
        "sha256:new",
        &subject,
        &env,
        "reports"
    ));
}

#[test]
fn decision_cache_is_scoped_to_environment() {
    let policies = engine(r#"ALLOW user FOR "regional" WHEN env.country_code == "IT";"#);
    let subject = user_subject(None, None);
    let italy = Env {
        country_code: "IT".into(),
        ..Default::default()
    };
    let france = Env {
        country_code: "FR".into(),
        ..Default::default()
    };

    assert!(access_control(
        &policies,
        "sha256:same",
        &subject,
        &italy,
        "regional"
    ));
    assert!(!access_control(
        &policies,
        "sha256:same",
        &subject,
        &france,
        "regional"
    ));
}
