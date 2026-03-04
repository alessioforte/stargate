use serde::{Deserialize, Serialize};
use std::fmt::Write;

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Endpoint {
    pub host: String,
    pub port: Option<i32>,
    pub path: Option<String>,
}

impl Endpoint {
    pub fn format(&self) -> String {
        let mut result = self.host.clone();
        if let Some(port) = self.port {
            write!(result, ":{}", port).unwrap();
        }
        if let Some(ref path) = self.path {
            result.push_str(path);
        }
        result
    }
}
