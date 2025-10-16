pub mod access_control;
pub mod endpoint;
pub mod limit;
pub mod load_balancer;
pub mod service;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Config {
    pub access_control: Option<access_control::AccessControl>,
    pub services: Vec<service::Service>,
    pub limits: Option<Vec<limit::Limit>>,
}
