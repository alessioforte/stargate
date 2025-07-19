use serde::{Deserialize, Serialize};
use std::env;
use std::path::Path;
use std::sync::Arc;

#[derive(Default, Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoadBalancerStrategy {
    #[default]
    RoundRobin,
    Random,
    IpHash,
}

impl LoadBalancerStrategy {
    pub fn build(&self, service: &Service) -> Box<Arc<dyn lb::LoadBalancer + Send + Sync>> {
        let protocol = service
            .protocol
            .clone()
            .unwrap_or_else(|| "http".to_string());
        let upstreams = service
            .endpoints
            .iter()
            .map(|ep| {
                let base_url = format!("{}://{}", protocol, ep.format());
                lb::Upstream { base_url }
            })
            .collect::<Vec<_>>();
        match self {
            LoadBalancerStrategy::RoundRobin => Box::new(lb::RoundRobin::new(upstreams)),
            LoadBalancerStrategy::Random => Box::new(lb::Random::new(upstreams)),
            LoadBalancerStrategy::IpHash => Box::new(lb::IpHash::new(upstreams)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Route {
    pub path: String,
    pub method: String,
    pub auth_required: Option<bool>,
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

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Service {
    pub connect_timeout: Option<u64>,
    pub name: Option<String>,
    pub path: String,
    pub protocol: Option<String>,
    pub endpoints: Vec<Endpoint>,
    pub load_balancer: Option<LoadBalancerStrategy>,
    pub auth_required: Option<bool>,
    pub routes: Option<Vec<Route>>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Config {
    pub services: Vec<Service>,
}

impl Config {
    pub fn from_file() -> Self {
        let path = env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string());
        let filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
        if !Path::new(&path).exists() {
            std::fs::create_dir(&path).expect("Unable to create config directory");
        }
        if !Path::new(&format!("{}/{}", path, filename)).exists() {
            log::info!("Creating gate configuration yaml file");
            let config = Config::default();
            let config_str = serde_yml::to_string(&config).expect("Unable to serialize config");
            std::fs::write(format!("{}/{}", path, filename), config_str)
                .expect("Unable to write config file");
            return config;
        }
        let file = std::fs::read_to_string(format!("{}/{}", path, filename))
            .expect("Unable to read config file");
        let config: Config = serde_yml::from_str(&file).expect("Unable to parse config file");
        config
    }

    pub fn to_file(&self) {
        let path = env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string());
        let filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
        let config_str = serde_yml::to_string(&self).expect("Unable to serialize config");
        std::fs::write(format!("{}/{}", path, filename), config_str)
            .expect("Unable to write config file");
    }
}
