use super::endpoint::Endpoint;
use super::load_balancer::LoadBalancer;
use crate::protocol::Protocol;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnvProfile {
    None,
    Basic,
    Geo,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextConfig {
    pub env: Option<EnvProfile>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StreamingMode {
    Sse,
}

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
    pub cost: Option<u64>,
    pub context: Option<ContextConfig>,
    pub routes: Option<Vec<Route>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Route {
    pub path: String,
    pub method: String,
    pub auth_required: Option<bool>,
    pub resource: Option<String>,
    pub cost: Option<u64>,
    pub context: Option<ContextConfig>,
    pub streaming: Option<StreamingMode>,
}

fn default_protocol() -> Protocol {
    Protocol::Http
}
