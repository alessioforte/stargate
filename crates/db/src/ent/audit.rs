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

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "actor_type", rename_all = "snake_case")]
pub enum ActorType {
    Admin,
    AdminKey,
    User,
    ApiKey,
    System,
    Anonymous,
}

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Audit {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub actor_type: ActorType,
    pub actor_id: Option<String>,
    pub action: ActionType,
    pub resource: Option<String>,
    pub resource_id: Option<String>,
    pub request_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct AuditContext {
    actor_type: ActorType,
    actor_id: Option<String>,
    request_id: Option<String>,
    ip_address: Option<String>,
    user_agent: Option<String>,
    resource: Option<String>,
    resource_id: Option<String>,
    metadata: Option<serde_json::Value>,
}

impl AuditContext {
    pub fn new(actor_type: ActorType, actor_id: Option<String>) -> Self {
        AuditContext {
            actor_type,
            actor_id,
            request_id: None,
            ip_address: None,
            user_agent: None,
            resource: None,
            resource_id: None,
            metadata: None,
        }
    }

    pub fn system() -> Self {
        AuditContext {
            actor_type: ActorType::System,
            actor_id: None,
            request_id: None,
            ip_address: None,
            user_agent: None,
            resource: None,
            resource_id: None,
            metadata: None,
        }
    }

    pub fn anonymous() -> Self {
        AuditContext {
            actor_type: ActorType::Anonymous,
            actor_id: None,
            request_id: None,
            ip_address: None,
            user_agent: None,
            resource: None,
            resource_id: None,
            metadata: None,
        }
    }

    pub fn admin() -> Self {
        AuditContext {
            actor_type: ActorType::Admin,
            actor_id: None,
            request_id: None,
            ip_address: None,
            user_agent: None,
            resource: None,
            resource_id: None,
            metadata: None,
        }
    }

    pub fn user() -> Self {
        AuditContext {
            actor_type: ActorType::User,
            actor_id: None,
            request_id: None,
            ip_address: None,
            user_agent: None,
            resource: None,
            resource_id: None,
            metadata: None,
        }
    }

    pub fn api_key() -> Self {
        AuditContext {
            actor_type: ActorType::ApiKey,
            actor_id: None,
            request_id: None,
            ip_address: None,
            user_agent: None,
            resource: None,
            resource_id: None,
            metadata: None,
        }
    }

    pub fn with_actor(mut self, actor_type: ActorType, actor_id: Option<String>) -> Self {
        self.actor_type = actor_type;
        self.actor_id = actor_id;
        self
    }

    pub fn with_actor_id(mut self, actor_id: String) -> Self {
        self.actor_id = Some(actor_id);
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

    pub fn with_resource_id(mut self, resource_id: String) -> Self {
        self.resource_id = Some(resource_id);
        self
    }

    pub fn build_audit(&self, action: ActionType) -> Audit {
        let id = ulid::Ulid::new().to_string();
        Audit {
            id,
            timestamp: Utc::now(),
            actor_type: self.actor_type.clone(),
            actor_id: self.actor_id.clone(),
            action,
            resource: self.resource.clone(),
            resource_id: self.resource_id.clone(),
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
