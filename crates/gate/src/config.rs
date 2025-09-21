use lb::{BaseLoadBalancer, IpHash, Random, RoundRobin};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Default, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Config {
    pub access_control: Option<AccessControl>,
    pub services: Vec<Service>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Service {
    pub connect_timeout: Option<u64>,
    pub name: Option<String>,
    pub path: String,
    pub protocol: Option<String>,
    pub endpoints: Vec<Endpoint>,
    pub load_balancer: Option<LoadBalancer>,
    pub auth_required: Option<bool>,
    pub resource: Option<String>,
    pub routes: Option<Vec<Route>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Route {
    pub path: String,
    pub method: String,
    pub auth_required: Option<bool>,
    pub resource: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Endpoint {
    pub host: String,
    pub port: Option<i32>,
    pub path: Option<String>,
}

impl Endpoint {
    pub fn format(&self) -> String {
        let port = match self.port {
            Some(port) => format!(":{}", port),
            None => "".to_string(),
        };
        let path = self.path.clone().unwrap_or_default();
        format!("{}{}{}", self.host, port, path)
    }
}

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
        protocol: Option<String>,
        endpoints: &Vec<Endpoint>,
    ) -> Box<Arc<dyn lb::LoadBalancer + Send + Sync>> {
        let protocol = protocol.unwrap_or_else(|| "http".to_string());
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

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AccessControl {
    pub policy_file: Option<String>,
    pub policies: Option<String>,
}
