use super::endpoint::Endpoint;
use super::load_balancer::LoadBalancer;
use serde::{Deserialize, Serialize};

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
