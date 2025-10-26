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

#[derive(Default, Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct LoadBalancer {
    pub strategy: LoadBalancerStrategy,
    pub liveness_probe: Option<LivenessProbe>,
}

impl LoadBalancer {
    pub fn builder(
        &self,
        protocol: &str,
        endpoints: &Vec<Endpoint>,
    ) -> Box<Arc<dyn lb::LoadBalancer + Send + Sync>> {
        let upstreams = endpoints
            .iter()
            .map(|endpoint| {
                let base_url = format!("{}://{}", protocol, endpoint.format());
                let mut health_check_path = None;
                if self.liveness_probe.is_some() {
                    let health_check = self.liveness_probe.as_ref().unwrap();
                    health_check_path = Some(health_check.path.clone());
                }

                lb::Upstream::new(base_url, health_check_path)
            })
            .collect::<Vec<_>>();
        match self.strategy {
            LoadBalancerStrategy::RoundRobin => {
                Box::new(BaseLoadBalancer::new(RoundRobin::new(), upstreams))
            }
            LoadBalancerStrategy::Random => {
                Box::new(BaseLoadBalancer::new(Random::new(), upstreams))
            }
            LoadBalancerStrategy::IpHash => {
                Box::new(BaseLoadBalancer::new(IpHash::new(), upstreams))
            }
        }
    }
}
