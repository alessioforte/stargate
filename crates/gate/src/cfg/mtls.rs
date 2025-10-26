use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct MtlsConfig {
    pub ca_cert_path: String,
    pub client_cert_path: String,
    pub client_key_path: String,
}
