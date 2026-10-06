use crate::api::gateway::lifecycle::Execution;
use crate::etc::gate::{RuntimeSnapshot, test_support};
use gate::cfg::{Config, MtlsConfig, RuntimeConfig};
use std::{sync::Arc, time::Duration};

pub(super) fn runtime(
    url: &str,
    protocols: &[&str],
    timeout: &str,
    mtls: Option<MtlsConfig>,
) -> Arc<RuntimeSnapshot> {
    let mut value = serde_json::to_value(Config::default()).unwrap();
    value["mtls"] = serde_json::to_value(mtls).unwrap();
    value["http"] = serde_json::json!({
        "upstreams":{"socket":{"targets":[{"url":url}],"transport":{"protocols":protocols,"connect_timeout":timeout}}},
        "services":{"socket":{"kind":"load_balancer","upstream":"socket"}}
    });
    test_support::runtime(RuntimeConfig::from_raw(serde_json::from_value(value).unwrap()).unwrap())
}

pub(super) fn execution(runtime: &RuntimeSnapshot) -> Execution {
    Execution::new(
        Duration::from_secs(5),
        runtime.resources.shutdown.child_token(),
    )
}
