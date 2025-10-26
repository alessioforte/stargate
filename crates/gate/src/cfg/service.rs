use super::endpoint::Endpoint;
use super::load_balancer::LoadBalancer;
use crate::protocol::Protocol;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Service {
    pub name: String,
    pub path: String,
    #[serde(default = "default_protocol")]
    pub protocol: Protocol,
    pub endpoints: Vec<Endpoint>,
    pub connect_timeout: Option<u64>,
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

fn default_protocol() -> Protocol {
    Protocol::Http
}
