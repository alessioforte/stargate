use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AccessControl {
    pub policies_path: Option<String>,
    pub policies: Option<String>,
}
