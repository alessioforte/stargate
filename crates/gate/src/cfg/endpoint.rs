use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Endpoint {
    pub host: String,
    pub port: Option<i32>,
    pub path: Option<String>,
}

impl Endpoint {
    pub fn format(&self) -> String {
        let port = match self.port {
            Some(port) => format!(":{}", port),
            None => "".to_string(),
        };
        let path = self.path.clone().unwrap_or_default();
        format!("{}{}{}", self.host, port, path)
    }
}
