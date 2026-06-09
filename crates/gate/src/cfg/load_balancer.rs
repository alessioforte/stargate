use super::endpoint::Endpoint;
use lb::{BaseLoadBalancer, IpHash, Random, RoundRobin};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Default, Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoadBalancerStrategy {
    #[default]
    RoundRobin,
    Random,
    IpHash,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct LivenessProbe {
    pub path: String,
    pub interval: Option<String>,
    pub timeout: Option<String>,
}

/// Per-upstream circuit-breaker tuning. Omitted fields fall back to defaults
/// (3 consecutive failures, 30s cooldown).
#[derive(Default, Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CircuitBreakerConfig {
    /// Consecutive failures before the circuit opens.
    pub fail_threshold: Option<usize>,
    /// Duration string (e.g. "30s") to wait before probing a recovered upstream.
    pub cooldown: Option<String>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct LoadBalancer {
    pub strategy: LoadBalancerStrategy,
    pub liveness_probe: Option<LivenessProbe>,
    pub circuit_breaker: Option<CircuitBreakerConfig>,
}

const DEFAULT_FAIL_THRESHOLD: usize = 3;
const DEFAULT_COOLDOWN_SECS: u64 = 30;

impl LoadBalancer {
    pub fn builder(
        &self,
        protocol: &str,
        endpoints: &[Endpoint],
    ) -> Arc<dyn lb::LoadBalancer + Send + Sync> {
        let urls = endpoints
            .iter()
            .map(|endpoint| format!("{}://{}", protocol, endpoint.format()))
            .collect::<Vec<_>>();
        self.builder_from_urls(&urls)
    }

    pub fn builder_from_urls(&self, urls: &[String]) -> Arc<dyn lb::LoadBalancer + Send + Sync> {
        let (fail_threshold, cooldown_secs) = self.circuit_breaker_params();
        let upstreams = urls
            .iter()
            .map(|base_url| {
                let health_check_path =
                    self.liveness_probe.as_ref().map(|probe| probe.path.clone());

                lb::Upstream::with_circuit_breaker(
                    base_url.clone(),
                    health_check_path,
                    fail_threshold,
                    cooldown_secs,
                )
            })
            .collect::<Vec<_>>();
        match self.strategy {
            LoadBalancerStrategy::RoundRobin => BaseLoadBalancer::new(RoundRobin::new(), upstreams),
            LoadBalancerStrategy::Random => BaseLoadBalancer::new(Random::new(), upstreams),
            LoadBalancerStrategy::IpHash => BaseLoadBalancer::new(IpHash::new(), upstreams),
        }
    }

    /// Resolve circuit-breaker threshold and cooldown, applying defaults for
    /// absent or invalid values (zero threshold, negative/unparsable cooldown).
    fn circuit_breaker_params(&self) -> (usize, u64) {
        let Some(cb) = &self.circuit_breaker else {
            return (DEFAULT_FAIL_THRESHOLD, DEFAULT_COOLDOWN_SECS);
        };
        let fail_threshold = cb
            .fail_threshold
            .filter(|threshold| *threshold > 0)
            .unwrap_or(DEFAULT_FAIL_THRESHOLD);
        let cooldown_secs = cb
            .cooldown
            .as_deref()
            .and_then(|raw| tools::parse_duration(raw).ok())
            .and_then(|duration| u64::try_from(duration.num_seconds()).ok())
            .unwrap_or(DEFAULT_COOLDOWN_SECS);
        (fail_threshold, cooldown_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lb_with(circuit_breaker: Option<CircuitBreakerConfig>) -> LoadBalancer {
        LoadBalancer {
            strategy: LoadBalancerStrategy::RoundRobin,
            liveness_probe: None,
            circuit_breaker,
        }
    }

    #[test]
    fn defaults_when_circuit_breaker_absent() {
        assert_eq!(lb_with(None).circuit_breaker_params(), (3, 30));
    }

    #[test]
    fn uses_configured_threshold_and_cooldown() {
        let cb = CircuitBreakerConfig {
            fail_threshold: Some(5),
            cooldown: Some("1m".to_string()),
        };
        assert_eq!(lb_with(Some(cb)).circuit_breaker_params(), (5, 60));
    }

    #[test]
    fn falls_back_on_zero_threshold_and_unparsable_cooldown() {
        let cb = CircuitBreakerConfig {
            fail_threshold: Some(0),
            cooldown: Some("nonsense".to_string()),
        };
        assert_eq!(lb_with(Some(cb)).circuit_breaker_params(), (3, 30));
    }

    #[test]
    fn partial_config_keeps_other_default() {
        let cb = CircuitBreakerConfig {
            fail_threshold: Some(7),
            cooldown: None,
        };
        assert_eq!(lb_with(Some(cb)).circuit_breaker_params(), (7, 30));
    }
}
