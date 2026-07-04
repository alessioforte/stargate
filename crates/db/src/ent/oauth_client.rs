use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthClient {
    pub client_id: String,
    #[serde(skip_serializing)]
    pub client_secret_hash: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub token_endpoint_auth_method: String,
    pub grant_types: sqlx::types::Json<Vec<String>>,
    pub response_types: sqlx::types::Json<Vec<String>>,
    pub redirect_uris: sqlx::types::Json<Vec<String>>,
    pub scopes: sqlx::types::Json<Vec<String>>,
    pub audiences: sqlx::types::Json<Vec<String>>,
    pub attrs: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OAuthClient {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client_id: String,
        client_secret_hash: Option<String>,
        name: String,
        description: Option<String>,
        token_endpoint_auth_method: String,
        grant_types: Vec<String>,
        response_types: Vec<String>,
        redirect_uris: Vec<String>,
        scopes: Vec<String>,
        audiences: Vec<String>,
        attrs: Value,
    ) -> Self {
        let now = Utc::now();
        Self {
            client_id,
            client_secret_hash,
            name,
            description,
            enabled: true,
            token_endpoint_auth_method,
            grant_types: sqlx::types::Json(grant_types),
            response_types: sqlx::types::Json(response_types),
            redirect_uris: sqlx::types::Json(redirect_uris),
            scopes: sqlx::types::Json(scopes),
            audiences: sqlx::types::Json(audiences),
            attrs,
            created_at: now,
            updated_at: now,
        }
    }
}
