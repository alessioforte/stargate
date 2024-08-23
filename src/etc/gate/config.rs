use serde::{Deserialize, Serialize};
use std::env;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub path: String,
    pub method: String,
    pub auth_required: Option<bool>,
    // created_at: Option<DateTime<Utc>>,
    // updated_at: Option<DateTime<Utc>>,
    // hosts: Option<Vec<String>>,
    // headers: Option<HashMap<String, String>>,
    // methods: Option<Vec<String>>,
    // paths: Option<Vec<String>>,
    // preserve_host: Option<bool>,
    // priority: Option<i32>,
    // protocols: Option<Vec<String>>,
    // strip_path: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub connect_timeout: Option<i32>,
    pub name: Option<String>,
    pub protocol: String,
    pub host: String,
    pub port: Option<i32>,
    pub path: String,
    pub auth_required: Option<bool>,
    pub routes: Option<Vec<Route>>,
    // created_at: Option<DateTime<Utc>>,
    // updated_at: Option<DateTime<Utc>>,
    // retries: Option<i32>,
    // read_timeout: Option<i32>,
    // write_timeout: Option<i32>,
    //
    // TODO: given an url, parse it and extract the protocol, host, port, path
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    pub services: Vec<Service>,
}

impl Config {
    pub fn from_file() -> Self {
        let path = env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string());
        let filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
        if !Path::new(".stargate").exists() {
            std::fs::create_dir(".stargate").expect("Unable to create config directory");
        }
        if !Path::new(&format!("{}/{}", path, filename)).exists() {
            log::info!("Creating gate configuration yaml file");
            let config = Config::default();
            let config_str = serde_yaml::to_string(&config).expect("Unable to serialize config");
            std::fs::write(format!("{}/{}", path, filename), config_str)
                .expect("Unable to write config file");
            return config;
        }
        let file = std::fs::read_to_string(format!("{}/{}", path, filename))
            .expect("Unable to read config file");
        let config: Config = serde_yaml::from_str(&file).expect("Unable to parse config file");
        config
    }

    pub fn to_file(&self) {
        let path = env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string());
        let filename = env::var("CONFIG_FILENAME").unwrap_or_else(|_| "config.yaml".to_string());
        let config_str = serde_yaml::to_string(&self).expect("Unable to serialize config");
        std::fs::write(format!("{}/{}", path, filename), config_str)
            .expect("Unable to write config file");
    }
}
