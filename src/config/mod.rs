use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

pub struct Config {
    pub services: HashMap<String, Service>,
}

pub fn get_service(path: &str, services: &HashMap<String, Service>) -> Option<Service> {
    let mut service = None;
    for (key, value) in services {
        if path.starts_with(key) {
            service = Some(value.clone());
            break;
        }
    }
    service
}

pub fn save_config_into_hashmap(config: &ConfigYAML) -> HashMap<String, Service> {
    let mut services = HashMap::new();
    for service in &config.services {
        services.insert(service.path.clone(), service.clone());
    }
    services
}
