pub mod access_control;
pub mod endpoint;
pub mod limit;
pub mod load_balancer;
pub mod mtls;
pub mod service;

use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Default, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Config {
    pub services: Vec<service::Service>,
    pub limits: Option<Vec<limit::Limit>>,
    pub access_control: Option<access_control::AccessControl>,
    pub mtls: Option<mtls::MtlsConfig>,
}

impl Config {
    pub fn from_file(path: &str) -> Self {
        if !std::path::Path::new(path).exists() {
            info!("Creating gate configuration yaml file");
            let config = Config::default();
            let config_str = serde_yaml_bw::to_string(&config).expect("Unable to serialize config");
            std::fs::write(path, config_str).expect("Unable to write config file");
            return config;
        }
        let file = std::fs::read_to_string(path).expect("Unable to read config file");
        serde_yaml_bw::from_str(&file).expect("Unable to parse config file")
    }

    pub fn to_file(&self, path: &str) {
        let config_str = serde_yaml_bw::to_string(&self).expect("Unable to serialize config");
        std::fs::write(path, config_str).expect("Unable to write config file");
    }
}
