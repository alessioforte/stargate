use super::{Gate, RuntimeSnapshot};
use opentelemetry::KeyValue;
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn targets(snapshot: &RuntimeSnapshot) -> BTreeMap<String, lb::TargetCounts> {
    let mut counts = BTreeMap::new();
    for (service_name, balancer) in &snapshot.core.balancers {
        if let Some(gate::graph::ServiceNode::LoadBalancer { upstream, .. }) =
            snapshot.core.graph.services.get(service_name)
        {
            // Multiple leaf services may share a balancer: report each upstream once.
            counts
                .entry(upstream.clone())
                .or_insert_with(|| balancer.target_counts());
        }
    }
    counts
}

pub(super) fn register(gate: &Arc<Gate>) {
    let gate = Arc::downgrade(gate);
    crate::obs::telemetry::meter()
        .u64_observable_gauge("stargate.gateway.upstream.targets")
        .with_description("Active generation targets by upstream and circuit state")
        .with_callback(move |observer| {
            let Some(gate) = gate.upgrade() else {
                return;
            };
            let snapshot = gate.snapshot();
            for (upstream, counts) in targets(&snapshot) {
                for (state, count) in [
                    ("healthy", counts.healthy),
                    ("unhealthy", counts.unhealthy),
                    ("circuit_open", counts.circuit_open),
                ] {
                    observer.observe(
                        count,
                        &[
                            KeyValue::new("stargate.upstream", upstream.to_owned()),
                            KeyValue::new("stargate.state", state),
                        ],
                    );
                }
            }
        })
        .build();
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn target_samples_deduplicate_services_and_follow_the_current_generation() {
        let mut config = serde_json::to_value(gate::cfg::Config::default()).unwrap();
        config["http"] = serde_json::json!({
            "upstreams": {"backend": {"targets": [{"url":"http://127.0.0.1:8080"}, {"url":"http://127.0.0.1:8081"}]}},
            "services": {"one": {"kind":"load_balancer", "upstream":"backend"}, "two": {"kind":"load_balancer", "upstream":"backend"}}
        });
        let gate = crate::gate::test_support::gate(serde_json::from_value(config).unwrap());
        let pinned = gate.snapshot();
        let first = targets(&pinned);
        assert_eq!(first.len(), 1);
        assert_eq!(first["backend"].healthy, 2);
        for _ in 0..3 {
            pinned.core.balancers["one"].mark_dead("http://127.0.0.1:8080");
        }
        let sample = targets(&gate.snapshot());
        assert_eq!(
            sample["backend"],
            lb::TargetCounts {
                healthy: 1,
                circuit_open: 1,
                unhealthy: 0
            }
        );
        gate.activate(crate::gate::prepare_config(gate::cfg::Config::default()).unwrap())
            .unwrap();
        assert!(targets(&gate.snapshot()).is_empty());
        assert_eq!(targets(&pinned)["backend"].circuit_open, 1);
    }
}
