use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "action_type", rename_all = "snake_case")]
pub enum ActionType {
    Create,
    Update,
    Delete,
    Read,
    Login,
    Logout,
}

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Audit {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub resource: Option<String>,
    pub action: ActionType,
    pub account_id: String,
    pub request_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct AuditContext {
    account_id: String,
    request_id: Option<String>,
    ip_address: Option<String>,
    user_agent: Option<String>,
    resource: Option<String>,
    metadata: Option<serde_json::Value>,
}

impl AuditContext {
    pub fn new(account_id: String) -> Self {
        AuditContext {
            account_id,
            request_id: None,
            ip_address: None,
            user_agent: None,
            resource: None,
            metadata: None,
        }
    }

    pub fn system() -> Self {
        AuditContext {
            account_id: "system".to_string(),
            request_id: None,
            ip_address: None,
            user_agent: None,
            resource: None,
            metadata: None,
        }
    }

    pub fn anonymous() -> Self {
        AuditContext {
            account_id: "anonymous".to_string(),
            request_id: None,
            ip_address: None,
            user_agent: None,
            resource: None,
            metadata: None,
        }
    }

    pub fn with_account_id(mut self, account_id: String) -> Self {
        self.account_id = account_id;
        self
    }

    pub fn with_request_context(
        mut self,
        request_id: String,
        ip_address: String,
        user_agent: String,
    ) -> Self {
        self.request_id = Some(request_id);
        self.ip_address = Some(ip_address);
        self.user_agent = Some(user_agent);
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn with_resource(mut self, resource: String) -> Self {
        self.resource = Some(resource);
        self
    }

    pub fn build_audit(&self, action: ActionType) -> Audit {
        let id = ulid::Ulid::new().to_string();
        Audit {
            id,
            action,
            timestamp: Utc::now(),
            resource: self.resource.clone(),
            account_id: self.account_id.clone(),
            request_id: self.request_id.clone(),
            ip_address: self.ip_address.clone(),
            user_agent: self.user_agent.clone(),
            metadata: self
                .metadata
                .clone()
                .unwrap_or_else(|| serde_json::json!({})),
        }
    }
}
