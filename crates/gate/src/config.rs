use serde::{Deserialize, Serialize};
use std::env;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Route {
    pub path: String,
    pub method: String,
    pub auth_required: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Uri {
    pub protocol: Option<String>,
    pub host: String,
    pub port: Option<i32>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Service {
    pub connect_timeout: Option<i32>,
    pub name: Option<String>,
    pub path: String,
    pub uri: Uri,
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
