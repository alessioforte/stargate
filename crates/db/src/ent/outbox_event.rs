use chrono::{DateTime, Utc};

#[derive(sqlx::FromRow, Debug, Clone, Eq, PartialEq)]
pub struct OutboxEventRow {
    pub event_id: String,
    pub payload: String,
    pub seq: i64,
    pub operation_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct OutboxEventFilter {
    pub event_id: Option<String>,
    pub operation_id: Option<String>,
    pub published: Option<bool>,
}

/// Global unpublished outbox backlog, without loading event payloads.
#[derive(sqlx::FromRow, Debug, Clone, PartialEq, Eq)]
pub struct OutboxBacklog {
    pub pending_events: i64,
    pub oldest_pending: Option<DateTime<Utc>>,
}
