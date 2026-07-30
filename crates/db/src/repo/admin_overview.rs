use crate::ent::AdminOverviewStats;
use anyhow::Result;

#[derive(Clone, Default)]
pub struct AdminOverviewRepository;

impl AdminOverviewRepository {
    pub fn new() -> Self {
        Self
    }

    pub async fn get<'c, E>(&self, ex: E) -> Result<AdminOverviewStats>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let stats = sqlx::query_as::<_, AdminOverviewStats>(
            r#"
            SELECT
                (SELECT COUNT(*) FROM users) AS users_total,
                (SELECT COUNT(*) FROM organizations) AS organizations_total,
                (SELECT COUNT(*) FROM service_accounts) AS service_accounts_total,
                (SELECT COUNT(*) FROM oauth_clients) AS oauth_clients_total,
                (SELECT COUNT(*) FROM oauth_clients WHERE enabled = TRUE) AS oauth_clients_enabled,
                (SELECT COUNT(*) FROM api_keys) AS api_keys_total,
                (SELECT COUNT(*) FROM api_keys WHERE revoked = TRUE) AS api_keys_revoked,
                (SELECT COUNT(*) FROM admin_keys) AS admin_keys_total,
                (SELECT COUNT(*) FROM admin_keys WHERE revoked = TRUE) AS admin_keys_revoked,
                (SELECT COUNT(*) FROM outbox_events WHERE published_at IS NULL) AS audit_pending,
                (SELECT MIN(created_at) FROM outbox_events WHERE published_at IS NULL)
                    AS audit_oldest_pending_at
            "#,
        )
        .fetch_one(ex)
        .await?;

        Ok(stats)
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn overview_returns_resource_breakdowns_and_oldest_pending_event() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        for statement in [
            "CREATE TABLE users (id TEXT PRIMARY KEY)",
            "CREATE TABLE organizations (id TEXT PRIMARY KEY)",
            "CREATE TABLE service_accounts (id TEXT PRIMARY KEY)",
            "CREATE TABLE oauth_clients (client_id TEXT PRIMARY KEY, enabled BOOLEAN NOT NULL)",
            "CREATE TABLE api_keys (id TEXT PRIMARY KEY, revoked BOOLEAN NOT NULL)",
            "CREATE TABLE admin_keys (id TEXT PRIMARY KEY, revoked BOOLEAN NOT NULL)",
            "CREATE TABLE outbox_events (
                event_id TEXT PRIMARY KEY,
                created_at TIMESTAMP NOT NULL,
                published_at TIMESTAMP NULL
            )",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }

        for statement in [
            "INSERT INTO users (id) VALUES ('u1'), ('u2')",
            "INSERT INTO organizations (id) VALUES ('o1')",
            "INSERT INTO service_accounts (id) VALUES ('s1')",
            "INSERT INTO oauth_clients (client_id, enabled) VALUES ('c1', TRUE), ('c2', FALSE)",
            "INSERT INTO api_keys (id, revoked) VALUES ('k1', FALSE), ('k2', TRUE)",
            "INSERT INTO admin_keys (id, revoked) VALUES ('a1', FALSE)",
            "INSERT INTO outbox_events (event_id, created_at, published_at)
                VALUES
                    ('e1', '2026-01-01T00:00:00Z', NULL),
                    ('e2', '2026-01-02T00:00:00Z', NULL),
                    ('e3', '2025-01-01T00:00:00Z', '2025-01-01T00:01:00Z')",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }

        let stats = AdminOverviewRepository::new().get(&pool).await.unwrap();
        assert_eq!(stats.users_total, 2);
        assert_eq!(stats.organizations_total, 1);
        assert_eq!(stats.service_accounts_total, 1);
        assert_eq!(stats.oauth_clients_total, 2);
        assert_eq!(stats.oauth_clients_enabled, 1);
        assert_eq!(stats.api_keys_total, 2);
        assert_eq!(stats.api_keys_revoked, 1);
        assert_eq!(stats.admin_keys_total, 1);
        assert_eq!(stats.admin_keys_revoked, 0);
        assert_eq!(stats.audit_pending, 2);
        assert_eq!(
            stats.audit_oldest_pending_at.unwrap().to_rfc3339(),
            "2026-01-01T00:00:00+00:00"
        );
    }
}
