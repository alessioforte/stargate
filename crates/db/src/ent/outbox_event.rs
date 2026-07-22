use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum OutboxPairRole {
    ControlPlane,
}

impl OutboxPairRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ControlPlane => "control_plane",
        }
    }
}

#[derive(sqlx::FromRow, Debug, Clone, Eq, PartialEq)]
pub struct OutboxEventRow {
    pub event_id: String,
    pub payload: String,
    pub seq: i64,
    pub operation_id: Option<String>,
    pub pair_role: Option<String>,
    pub created_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
}
