use super::{
    Gate, load_config_from_path, reload_gateway_config_from_path, reload_policy_engine_from_path,
};
use gate::{
    PolicySnapshot,
    cfg::{RuntimeConfig, SCHEMA},
};
use std::{fs, sync::Arc};

fn policy_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "stargate-policy-reload-{}-{}",
        std::process::id(),
        ulid::Ulid::generate()
    ))
}

#[tokio::test]
async fn unsupported_schema_reload_keeps_the_active_runtime() {
    let path = policy_path();
    let path_str = path.to_str().unwrap();
    fs::write(
        &path,
        r#"
schema: stargate/v1
ingress:
  limit: ingress
  timeout: 250ms
limits:
  default:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 1s
  ingress:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 100ms
http:
  services:
    healthy:
      kind: direct_response
      status: 200
      body:
        text: ready
  routers:
    healthy:
      match:
        path:
          prefix: /
      service: healthy
"#,
    )
    .unwrap();
    let prepared = load_config_from_path(path_str).unwrap();
    let config = prepared.config.clone();
    let gate = Gate::new(
        Arc::new(lim::State::new()),
        prepared,
        PolicySnapshot::default(),
    )
    .unwrap();
    let runtime = gate.snapshot();
    assert_eq!(runtime.core.graph.routers[0].service, "healthy");

    for yaml in [
        "schema: stargate/v2alpha1\nhttp: {}\n",
        "schema: stargate/v2\nhttp: {}\n",
        "http: {}\n",
    ] {
        fs::write(&path, yaml).unwrap();
        assert!(load_config_from_path(path_str).is_err());
        let error = reload_gateway_config_from_path(&gate, path_str).unwrap_err();
        assert!(error.to_string().contains(SCHEMA));
        assert!(Arc::ptr_eq(&gate.snapshot(), &runtime));
        assert_eq!(gate.snapshot().version, 0);
    }
    assert_eq!(
        RuntimeConfig::from_raw(config.raw).unwrap().compiled.schema,
        SCHEMA
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn invalid_policy_reload_keeps_the_active_snapshot() {
    let path = policy_path();
    let path_str = path.to_str().unwrap();
    let gate = crate::etc::gate::test_support::gate(gate::cfg::Config::default());
    let runtime = gate.snapshot();

    fs::write(&path, "ALLOW user FOR \"reports:READ\";").unwrap();
    assert!(reload_policy_engine_from_path(&gate, path_str).unwrap());
    let initial_revision = gate.policy_snapshot.load().revision.clone();
    assert!(!reload_policy_engine_from_path(&gate, path_str).unwrap());

    fs::write(&path, "ALLOW user WHEN invalid;").unwrap();
    assert!(reload_policy_engine_from_path(&gate, path_str).is_err());
    assert_eq!(gate.policy_snapshot.load().revision, initial_revision);

    fs::write(&path, "ALLOW user FOR \"dashboard\";").unwrap();
    assert!(reload_policy_engine_from_path(&gate, path_str).unwrap());
    assert_ne!(gate.policy_snapshot.load().revision, initial_revision);

    assert!(Arc::ptr_eq(&runtime, &gate.snapshot()));
    assert_eq!(gate.snapshot().version, 0);
    fs::remove_file(path).unwrap();
}
