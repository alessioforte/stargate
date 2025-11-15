use chrono::{DateTime, Utc};
use objectid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "action_type", rename_all = "snake_case")]
pub enum ActionType {
    Create,
    Read,
    Update,
    Delete,
    Login,
    Logout,
    PasswordChange,
    PermissionChange,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "actor_type", rename_all = "snake_case")]
pub enum ActorType {
    User,
    Service,
    System,
    Anonymous,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "event_type", rename_all = "snake_case")]
pub enum EventType {
    Authentication,
    Authorization,
    DataAccess,
    DataModification,
    SystemEvent,
    SecurityEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "outcome_type", rename_all = "snake_case")]
pub enum OutcomeType {
    Success,
    Failure,
    PartialSuccess,
}

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Audit {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub event_type: EventType,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub resource_name: Option<String>,
    pub action_type: String,
    pub actor_type: ActorType,
    pub actor_name: String,
    pub actor_id: String,
    pub outcome: OutcomeType,
    pub metadata: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

pub struct Actor {
    id: String,
    actor_type: ActorType,
    name: String,
}

pub struct Resource {
    resource_type: String,
    id: Option<String>,
    name: Option<String>,
}

pub struct AuditBuilder {
    event_type: EventType,
    actor: Actor,
    resource: Resource,
    outcome: OutcomeType,
    action: String,
    metadata: Option<serde_json::Value>,
    ip_address: Option<String>,
    user_agent: Option<String>,
}

impl AuditBuilder {
    pub fn new(
        event_type: EventType,
        actor: Actor,
        action: String,
        resource: Resource,
        outcome: OutcomeType,
    ) -> Self {
        AuditBuilder {
            event_type,
            actor,
            resource,
            outcome,
            action,
            metadata: None,
            ip_address: None,
            user_agent: None,
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn with_request_context(mut self, ip_address: String, user_agent: String) -> Self {
        self.ip_address = Some(ip_address);
        self.user_agent = Some(user_agent);
        self
    }

    pub fn build(self) -> Audit {
        Audit {
            id: ObjectId::new().unwrap().to_string(),
            timestamp: Utc::now(),
            event_type: self.event_type,
            resource_type: self.resource.resource_type,
            resource_id: self.resource.id,
            resource_name: self.resource.name,
            action_type: self.action,
            actor_type: self.actor.actor_type,
            actor_name: self.actor.name,
            actor_id: self.actor.id,
            outcome: self.outcome,
            metadata: self.metadata,
            ip_address: self.ip_address,
            user_agent: self.user_agent,
        }
    }
}
