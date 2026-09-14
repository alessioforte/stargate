use super::user::User;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub attrs: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Organization {
    pub fn new(name: String, description: Option<String>) -> Self {
        let id = ulid::Ulid::generate().to_string();
        let now = Utc::now();
        Organization {
            id,
            name,
            description,
            attrs: Value::Object(serde_json::Map::new()),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn attrs(mut self, attrs: Value) -> Self {
        self.attrs = attrs;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewOrganizationMembership {
    pub organization_id: String,
    pub role: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum OrganizationMembershipMutation {
    Create,
    Update,
}

/// An organization a user belongs to, with the membership metadata.
///
/// `member_since` is nullable because sqlite rows created before the
/// multi-org migration are backfilled, but the column itself stays nullable.
#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgMembership {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub organization: Organization,
    pub role: String,
    pub member_since: Option<DateTime<Utc>>,
}

/// A user belonging to an organization, with the membership metadata.
#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgMember {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub user: User,
    pub role: String,
    pub member_since: Option<DateTime<Utc>>,
}
