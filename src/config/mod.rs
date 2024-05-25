use serde::{Deserialize, Serialize};
use serde_yaml;
use std::collections::HashMap;
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub path: String,
    pub method: String,
    pub auth_required: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub connect_timeout: Option<i32>,
    pub name: Option<String>,
    pub protocol: String,
    pub host: String,
    pub port: String,
    pub path: String,
    pub auth_required: Option<bool>,
    pub routes: Option<Vec<Route>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigYAML {
    pub services: Vec<Service>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub services: HashMap<String, Service>,
}

impl Config {
    pub fn new(yaml: &ConfigYAML) -> Config {
        let mut services = HashMap::new();
        for service in &yaml.services {
            services.insert(service.path.clone(), service.clone());
        }
        Config { services }
    }

    pub fn get_service(&self, path: &str) -> Option<Service> {
        let mut service = None;
        for (key, value) in &self.services {
            if path.starts_with(key) {
                service = Some(value.clone());
                break;
            }
        }
        service
    }

    pub fn export(&self) -> ConfigYAML {
        let mut services = vec![];
        for (_, value) in &self.services {
            services.push(value.clone());
        }
        ConfigYAML { services }
    }
}

pub fn get_config_from_yaml() -> Config {
    let path = env::var("CONFIG_PATH").unwrap_or_else(|_| "".to_string());
    let filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
    let file = std::fs::read_to_string(format!("{}/{}", path, filename))
        .expect("Unable to read config file");
    let yaml: ConfigYAML = serde_yaml::from_str(&file).expect("Unable to parse config file");
    Config::new(&yaml)
}
