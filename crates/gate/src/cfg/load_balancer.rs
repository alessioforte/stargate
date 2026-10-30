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
    #[serde(default)]
    pub protocol: ProbeProtocol,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    pub interval: Option<String>,
    /// Positive connection and total request timeout; defaults to 2s.
    pub timeout: Option<String>,
}

impl LivenessProbe {
    pub fn timeout(&self) -> Result<std::time::Duration, &'static str> {
        let invalid = "liveness timeout must be a positive, representable duration";
        let duration = tools::parse_duration(self.timeout.as_deref().unwrap_or("2s"))
            .map_err(|_| invalid)?
            .to_std()
            .map_err(|_| invalid)?;
        if duration.is_zero() || std::time::Instant::now().checked_add(duration).is_none() {
            return Err(invalid);
        }
        Ok(duration)
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProbeProtocol {
    #[default]
    Http,
    Grpc,
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
    pub(crate) fn probe_interval(&self) -> Result<Option<chrono::Duration>, &'static str> {
        let Some(probe) = &self.liveness_probe else {
            return Ok(None);
        };
        let interval = tools::parse_duration(probe.interval.as_deref().unwrap_or("5s"))
            .map_err(|_| "liveness interval must be a positive, representable duration")?;
        let duration = interval
            .to_std()
            .map_err(|_| "liveness interval must be a positive, representable duration")?;
        if duration.is_zero() || std::time::Instant::now().checked_add(duration).is_none() {
            return Err("liveness interval must be a positive, representable duration");
        }
        Ok(Some(interval))
    }

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
                let health_check = self
                    .liveness_probe
                    .as_ref()
                    .map(|probe| match probe.protocol {
                        ProbeProtocol::Http => lb::HealthProbe::Http {
                            path: probe.path.clone().unwrap_or_default(),
                        },
                        ProbeProtocol::Grpc => lb::HealthProbe::Grpc {
                            service: probe.service.clone().unwrap_or_default(),
                        },
                    });

                lb::Upstream::with_circuit_breaker(
                    base_url.clone(),
                    health_check,
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

    #[test]
    fn probe_timeout_defaults_to_two_seconds_and_accepts_positive_durations() {
        assert_eq!(
            LivenessProbe::default().timeout().unwrap(),
            std::time::Duration::from_secs(2)
        );
        let probe = LivenessProbe {
            timeout: Some("250ms".into()),
            ..LivenessProbe::default()
        };
        assert_eq!(
            probe.timeout().unwrap(),
            std::time::Duration::from_millis(250)
        );
    }

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
