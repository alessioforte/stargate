use chrono::{DateTime, Utc};

/// Database-backed counters used by the read-only admin overview.
#[derive(sqlx::FromRow, Debug, Clone, PartialEq, Eq)]
pub struct AdminOverviewStats {
    pub users_total: i64,
    pub organizations_total: i64,
    pub service_accounts_total: i64,
    pub oauth_clients_total: i64,
    pub oauth_clients_enabled: i64,
    pub api_keys_total: i64,
    pub api_keys_revoked: i64,
    pub admin_keys_total: i64,
    pub admin_keys_revoked: i64,
    pub audit_pending: i64,
    pub audit_oldest_pending_at: Option<DateTime<Utc>>,
}
