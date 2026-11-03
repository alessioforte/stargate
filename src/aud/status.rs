//! Cached relay diagnostics: health handlers and metric callbacks never query dependencies.
use once_cell::sync::Lazy;
use serde::Serialize;
use std::sync::RwLock;
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    ConsumerBacklog,
    GroupMissing,
}
impl PauseReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConsumerBacklog => "consumer_backlog",
            Self::GroupMissing => "group_missing",
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditStatus {
    pub relay: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<PauseReason>,
    pub pending_events: u64,
    pub oldest_pending_age_secs: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oldest_unconfirmed_age_secs: Option<f64>,
    pub updated_at: String,
}
impl Default for AuditStatus {
    fn default() -> Self {
        Self {
            relay: "disabled",
            pause_reason: None,
            pending_events: 0,
            oldest_pending_age_secs: 0.0,
            stream_length: None,
            oldest_unconfirmed_age_secs: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}
static STATUS: Lazy<RwLock<AuditStatus>> = Lazy::new(|| RwLock::new(AuditStatus::default()));
pub fn snapshot() -> AuditStatus {
    STATUS.read().unwrap_or_else(|e| e.into_inner()).clone()
}
pub(super) fn update(status: &AuditStatus) {
    let mut cached = STATUS.write().unwrap_or_else(|e| e.into_inner());
    *cached = status.clone();
    cached.updated_at = chrono::Utc::now().to_rfc3339();
}
