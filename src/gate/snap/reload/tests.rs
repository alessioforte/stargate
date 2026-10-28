use super::{apply_gateway_config, apply_policy_document};
use gate::cfg::{Config, SCHEMA};
use std::sync::Arc;

#[tokio::test]
async fn unsupported_schema_reload_keeps_the_active_runtime() {
    let gate = crate::gate::test_support::gate(Config::default());
    let runtime = gate.snapshot();
    for yaml in [
        "schema: stargate/v2alpha1\nhttp: {}\n",
        "schema: stargate/v2\nhttp: {}\n",
        "http: {}\n",
    ] {
        let error = apply_gateway_config(&gate, yaml).unwrap_err();
        assert!(error.to_string().contains(SCHEMA));
        assert!(Arc::ptr_eq(&gate.snapshot(), &runtime));
        assert_eq!(gate.snapshot().version, 0);
    }
}

#[test]
fn invalid_policy_reload_keeps_the_active_snapshot() {
    let gate = crate::gate::test_support::gate(Config::default());
    let runtime = gate.snapshot();
    assert!(apply_policy_document(&gate, "ALLOW user FOR \"reports:READ\";").unwrap());
    let initial_revision = gate.policy_snapshot.load().revision.clone();
    assert!(!apply_policy_document(&gate, "ALLOW user FOR \"reports:READ\";").unwrap());
    assert!(apply_policy_document(&gate, "ALLOW user WHEN invalid;").is_err());
    assert_eq!(gate.policy_snapshot.load().revision, initial_revision);
    assert!(apply_policy_document(&gate, "ALLOW user FOR \"dashboard\";").unwrap());
    assert_ne!(gate.policy_snapshot.load().revision, initial_revision);
    assert!(Arc::ptr_eq(&runtime, &gate.snapshot()));
    assert_eq!(gate.snapshot().version, 0);
}
