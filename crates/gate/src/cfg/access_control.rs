use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AccessControl {
    pub policy_file: Option<String>,
    pub policies: Option<String>,
}
