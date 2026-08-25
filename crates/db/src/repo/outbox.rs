use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::ent::{AuditId, OutboxEventFilter, OutboxEventRow, OutboxPairRole, ValidatedAuditEvent};

pub const OUTBOX_EVENT: &str = "outbox_events";

#[derive(Debug, Deserialize)]
struct PayloadIdentity {
    event_id: AuditId,
    operation_id: Option<AuditId>,
    scope: crate::ent::AuditScopeSelector,
}

#[derive(Debug, Clone, Default)]
pub struct OutboxRepository;

impl OutboxRepository {
    pub fn new() -> Self {
        Self
    }

    pub async fn insert(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        event: &ValidatedAuditEvent,
        pair_role: Option<OutboxPairRole>,
    ) -> Result<OutboxEventRow> {
        validate_pair_role(event, pair_role)?;
        verify_validated_payload_identity(event)?;

        let operation_id = event.operation_id().map(AuditId::as_str);
        let pair_role = pair_role.map(OutboxPairRole::as_str);
        let row = sqlx::query_as::<_, OutboxEventRow>(
            r#"
            INSERT INTO outbox_events (
                event_id,
                payload,
                operation_id,
                pair_role
            )
            VALUES ($1, $2, $3, $4)
            RETURNING
                event_id,
                payload,
                seq,
                operation_id,
                pair_role,
                created_at,
                published_at
            "#,
        )
        .bind(event.event_id().as_str())
        .bind(event.payload())
        .bind(operation_id)
        .bind(pair_role)
        .fetch_one(&mut **tx)
        .await?;

        verify_row_payload_identity(&row)?;
        Ok(row)
    }

    pub async fn get_by_event_id<'c, E>(
        &self,
        ex: E,
        event_id: &str,
    ) -> Result<Option<OutboxEventRow>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row = sqlx::query_as::<_, OutboxEventRow>(
            r#"
            SELECT
                event_id,
                payload,
                seq,
                operation_id,
                pair_role,
                created_at,
                published_at
            FROM outbox_events
            WHERE event_id = $1
            "#,
        )
        .bind(event_id)
        .fetch_optional(ex)
        .await?;

        if let Some(row) = &row {
            verify_row_payload_identity(row)?;
        }
        Ok(row)
    }

    pub async fn query<'c, E>(
        &self,
        ex: E,
        filter: &OutboxEventFilter,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<OutboxEventRow>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        ensure!(limit > 0, "outbox query limit must be greater than zero");
        ensure!(offset >= 0, "outbox query offset cannot be negative");

        let rows = sqlx::query_as::<_, OutboxEventRow>(
            r#"
            SELECT
                event_id,
                payload,
                seq,
                operation_id,
                pair_role,
                created_at,
                published_at
            FROM outbox_events
            WHERE ($1 IS NULL OR event_id = $1)
              AND ($2 IS NULL OR operation_id = $2)
              AND ($3 IS NULL OR pair_role = $3)
              AND (
                    $4 IS NULL
                    OR ($4 = TRUE AND published_at IS NOT NULL)
                    OR ($4 = FALSE AND published_at IS NULL)
                  )
            ORDER BY seq DESC
            LIMIT $5 OFFSET $6
            "#,
        )
        .bind(filter.event_id.as_deref())
        .bind(filter.operation_id.as_deref())
        .bind(filter.pair_role.as_deref())
        .bind(filter.published)
        .bind(limit)
        .bind(offset)
        .fetch_all(ex)
        .await?;

        for row in &rows {
            verify_row_payload_identity(row)?;
        }
        Ok(rows)
    }

    pub async fn count<'c, E>(&self, ex: E, filter: &OutboxEventFilter) -> Result<i64>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM outbox_events
            WHERE ($1 IS NULL OR event_id = $1)
              AND ($2 IS NULL OR operation_id = $2)
              AND ($3 IS NULL OR pair_role = $3)
              AND (
                    $4 IS NULL
                    OR ($4 = TRUE AND published_at IS NOT NULL)
                    OR ($4 = FALSE AND published_at IS NULL)
                  )
            "#,
        )
        .bind(filter.event_id.as_deref())
        .bind(filter.operation_id.as_deref())
        .bind(filter.pair_role.as_deref())
        .bind(filter.published)
        .fetch_one(ex)
        .await?;

        Ok(count)
    }
}

#[cfg(feature = "postgres")]
impl OutboxRepository {
    pub async fn claim_unpublished(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        limit: i64,
    ) -> Result<Vec<OutboxEventRow>> {
        ensure!(limit > 0, "outbox claim limit must be greater than zero");

        let rows = sqlx::query_as::<_, OutboxEventRow>(
            r#"
            SELECT
                event_id,
                payload,
                seq,
                operation_id,
                pair_role,
                created_at,
                published_at
            FROM outbox_events
            WHERE published_at IS NULL
            ORDER BY seq
            LIMIT $1
            FOR UPDATE SKIP LOCKED
            "#,
        )
        .bind(limit)
        .fetch_all(&mut **tx)
        .await?;

        for row in &rows {
            verify_row_payload_identity(row)?;
        }
        Ok(rows)
    }

    pub async fn mark_published(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        seqs: &[i64],
    ) -> Result<()> {
        if seqs.is_empty() {
            return Ok(());
        }

        sqlx::query("UPDATE outbox_events SET published_at = NOW() WHERE seq = ANY($1)")
            .bind(seqs)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }
}

fn validate_pair_role(
    event: &ValidatedAuditEvent,
    pair_role: Option<OutboxPairRole>,
) -> Result<()> {
    ensure!(
        event.operation_id().is_some() == pair_role.is_some(),
        "operation_id and pair_role must either both be present or both be absent"
    );
    if matches!(pair_role, Some(OutboxPairRole::ControlPlane)) {
        ensure!(
            matches!(
                event.event().scope,
                crate::ent::AuditScopeSelector::ControlPlane
            ),
            "control_plane pair_role requires a control_plane payload scope"
        );
    }
    Ok(())
}

fn verify_validated_payload_identity(event: &ValidatedAuditEvent) -> Result<()> {
    let identity = payload_identity(event.payload())?;
    ensure!(
        &identity.event_id == event.event_id(),
        "validated event_id does not match payload event_id"
    );
    ensure!(
        identity.operation_id.as_ref() == event.operation_id(),
        "validated operation_id does not match payload operation_id"
    );
    Ok(())
}

fn verify_row_payload_identity(row: &OutboxEventRow) -> Result<()> {
    let row_event_id =
        AuditId::parse(&row.event_id).context("outbox row contains an invalid event_id")?;
    let row_operation_id = row
        .operation_id
        .as_deref()
        .map(AuditId::parse)
        .transpose()
        .context("outbox row contains an invalid operation_id")?;
    let identity = payload_identity(&row.payload)?;

    ensure!(
        identity.event_id == row_event_id,
        "outbox row event_id does not match payload event_id"
    );
    ensure!(
        identity.operation_id == row_operation_id,
        "outbox row operation_id does not match payload operation_id"
    );
    match row.pair_role.as_deref() {
        Some("control_plane") => ensure!(
            matches!(identity.scope, crate::ent::AuditScopeSelector::ControlPlane),
            "outbox control_plane pair_role does not match payload scope"
        ),
        Some("target") => ensure!(
            !matches!(identity.scope, crate::ent::AuditScopeSelector::ControlPlane),
            "outbox target pair_role cannot contain a control_plane payload scope"
        ),
        Some(_) => anyhow::bail!("outbox row contains an invalid pair_role"),
        None => {}
    }
    Ok(())
}

fn payload_identity(payload: &str) -> Result<PayloadIdentity> {
    serde_json::from_str(payload).context("outbox payload identity is invalid")
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::ent::{
        AuditActor, AuditOperation, AuditResource, AuditScopeSelector, AuditService,
        RawAuditEventBuilder,
    };

    fn validated_event(event_id: &str, operation_id: Option<&str>) -> ValidatedAuditEvent {
        validated_event_in_scope(event_id, operation_id, AuditScopeSelector::control_plane())
    }

    fn validated_event_in_scope(
        event_id: &str,
        operation_id: Option<&str>,
        scope: AuditScopeSelector,
    ) -> ValidatedAuditEvent {
        let mut builder = RawAuditEventBuilder::new()
            .event_id(AuditId::parse(event_id).expect("test event id must be valid"))
            .occurred_at(
                Utc.with_ymd_and_hms(2026, 7, 19, 10, 0, 0)
                    .single()
                    .expect("test timestamp must be valid"),
            )
            .scope(scope)
            .actor(AuditActor::new("system"))
            .service(AuditService::stargate())
            .resource(AuditResource::new("user", "user-123"))
            .action("user.updated")
            .operation(AuditOperation::Update);
        if let Some(operation_id) = operation_id {
            builder = builder.operation_id(
                AuditId::parse(operation_id).expect("test operation id must be valid"),
            );
        }
        builder.build().expect("test event must validate")
    }

    #[test]
    fn audit_outbox_payload_identity_rejects_mismatched_event_id() {
        let event = validated_event("01JZ0000000000000000000001", None);
        let mismatched = event.payload().replacen(
            "01JZ0000000000000000000001",
            "01JZ0000000000000000000002",
            1,
        );
        let row = OutboxEventRow {
            event_id: event.event_id().to_string(),
            payload: mismatched,
            seq: 1,
            operation_id: None,
            pair_role: None,
            created_at: Utc::now(),
            published_at: None,
        };

        let error = verify_row_payload_identity(&row)
            .expect_err("mismatched row and payload ids must fail");
        assert!(error.to_string().contains("event_id does not match"));
    }

    #[test]
    fn audit_outbox_pair_role_requires_operation_id() {
        let unpaired = validated_event("01JZ0000000000000000000001", None);
        assert!(validate_pair_role(&unpaired, None).is_ok());
        assert!(validate_pair_role(&unpaired, Some(OutboxPairRole::ControlPlane)).is_err());

        let paired = validated_event(
            "01JZ0000000000000000000002",
            Some("01JZ000000000000000000000X"),
        );
        assert!(validate_pair_role(&paired, None).is_err());
        assert!(validate_pair_role(&paired, Some(OutboxPairRole::ControlPlane)).is_ok());

        let application_paired = validated_event_in_scope(
            "01JZ0000000000000000000003",
            Some("01JZ000000000000000000000X"),
            AuditScopeSelector::application(),
        );
        assert!(
            validate_pair_role(&application_paired, Some(OutboxPairRole::ControlPlane)).is_err()
        );

        let mismatched_row = OutboxEventRow {
            event_id: application_paired.event_id().to_string(),
            payload: application_paired.payload().to_string(),
            seq: 1,
            operation_id: application_paired.operation_id().map(AuditId::to_string),
            pair_role: Some("control_plane".to_string()),
            created_at: Utc::now(),
            published_at: None,
        };
        assert!(verify_row_payload_identity(&mismatched_row).is_err());
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod sqlite_tests {
    use chrono::{TimeZone, Utc};
    use sqlx::Row;

    use super::*;
    use crate::ent::{
        AuditActor, AuditOperation, AuditResource, AuditScopeSelector, AuditService,
        RawAuditEventBuilder,
    };

    const PRE_OUTBOX_MIGRATION: i64 = 20_260_712_000_000;
    static SQLITE_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations/sqlite");

    fn validated_event(event_id: &str, operation_id: Option<&str>) -> ValidatedAuditEvent {
        let mut builder = RawAuditEventBuilder::new()
            .event_id(AuditId::parse(event_id).expect("test event id must be valid"))
            .occurred_at(
                Utc.with_ymd_and_hms(2026, 7, 19, 10, 0, 0)
                    .single()
                    .expect("test timestamp must be valid"),
            )
            .scope(AuditScopeSelector::control_plane())
            .actor(AuditActor::new("system"))
            .service(AuditService::stargate())
            .resource(AuditResource::new("user", "user-123"))
            .action("user.updated")
            .operation(AuditOperation::Update);
        if let Some(operation_id) = operation_id {
            builder = builder.operation_id(
                AuditId::parse(operation_id).expect("test operation id must be valid"),
            );
        }
        builder.build().expect("test event must validate")
    }

    async fn empty_pool() -> crate::backend::Pool {
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite pool must connect")
    }

    async fn migrated_pool() -> crate::backend::Pool {
        let pool = empty_pool().await;
        SQLITE_MIGRATOR
            .run(&pool)
            .await
            .expect("SQLite migrations must apply");
        pool
    }

    async fn table_exists(pool: &crate::backend::Pool, table: &str) -> bool {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = $1",
        )
        .bind(table)
        .fetch_one(pool)
        .await
        .expect("SQLite schema lookup must succeed")
            == 1
    }

    #[tokio::test]
    async fn audit_outbox_sqlite_migrates_blank_schema() {
        let pool = migrated_pool().await;

        let columns = sqlx::query("PRAGMA table_info('outbox_events')")
            .fetch_all(&pool)
            .await
            .expect("outbox table info must be readable")
            .into_iter()
            .map(|row| row.get::<String, _>("name"))
            .collect::<Vec<_>>();
        assert_eq!(
            columns,
            vec![
                "seq",
                "event_id",
                "payload",
                "operation_id",
                "pair_role",
                "created_at",
                "published_at",
            ]
        );
        assert!(!table_exists(&pool, "audits").await);

        let indexes = sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'outbox_events'",
        )
        .fetch_all(&pool)
        .await
        .expect("outbox indexes must be readable");
        assert!(
            indexes
                .iter()
                .any(|name| name == "idx_outbox_events_unpublished")
        );
        assert!(
            indexes
                .iter()
                .any(|name| name == "idx_outbox_events_pair_role")
        );
    }

    #[tokio::test]
    async fn audit_outbox_sqlite_upgrades_pre_refactor_schema_and_drops_legacy_audits() {
        let pool = empty_pool().await;
        SQLITE_MIGRATOR
            .run_to(PRE_OUTBOX_MIGRATION, &pool)
            .await
            .expect("pre-outbox migrations must apply");
        assert!(!table_exists(&pool, OUTBOX_EVENT).await);

        SQLITE_MIGRATOR
            .run(&pool)
            .await
            .expect("outbox migration must apply after the current schema");
        assert!(table_exists(&pool, OUTBOX_EVENT).await);
        assert!(!table_exists(&pool, "audits").await);
    }

    #[tokio::test]
    async fn audit_outbox_sqlite_insert_preserves_exact_payload_and_pending_state() {
        let pool = migrated_pool().await;
        let repository = OutboxRepository::new();
        let event = validated_event("01JZ0000000000000000000001", None);
        let mut tx = pool.begin().await.expect("transaction must begin");

        let row = repository
            .insert(&mut tx, &event, None)
            .await
            .expect("validated event must insert");
        tx.commit().await.expect("transaction must commit");

        assert_eq!(row.event_id, event.event_id().as_str());
        assert_eq!(row.payload, event.payload());
        assert_eq!(row.seq, 1);
        assert_eq!(row.operation_id, None);
        assert_eq!(row.pair_role, None);
        assert_eq!(row.published_at, None);

        let stored = sqlx::query_as::<_, OutboxEventRow>(
            r#"
            SELECT
                event_id,
                payload,
                seq,
                operation_id,
                pair_role,
                created_at,
                published_at
            FROM outbox_events
            WHERE event_id = $1
            "#,
        )
        .bind(event.event_id().as_str())
        .fetch_one(&pool)
        .await
        .expect("stored event must be readable");
        assert_eq!(stored.payload, event.payload());
        assert_eq!(stored.published_at, None);
    }

    #[tokio::test]
    async fn audit_outbox_sqlite_enforces_payload_identity_and_size() {
        let pool = migrated_pool().await;
        let event = validated_event("01JZ0000000000000000000001", None);
        let mismatched = event.payload().replacen(
            "01JZ0000000000000000000001",
            "01JZ0000000000000000000002",
            1,
        );
        let mismatch_result =
            sqlx::query("INSERT INTO outbox_events (event_id, payload) VALUES ($1, $2)")
                .bind(event.event_id().as_str())
                .bind(mismatched)
                .execute(&pool)
                .await;
        assert!(mismatch_result.is_err());

        let oversized = serde_json::json!({
            "event_id": event.event_id().as_str(),
            "operation_id": null,
            "padding": "x".repeat(65_536),
        })
        .to_string();
        let oversized_result =
            sqlx::query("INSERT INTO outbox_events (event_id, payload) VALUES ($1, $2)")
                .bind(event.event_id().as_str())
                .bind(oversized)
                .execute(&pool)
                .await;
        assert!(oversized_result.is_err());
    }

    #[tokio::test]
    async fn audit_outbox_sqlite_enforces_operation_role_uniqueness() {
        let pool = migrated_pool().await;
        let repository = OutboxRepository::new();
        let operation_id = "01JZ000000000000000000000X";
        let control_plane = validated_event("01JZ0000000000000000000001", Some(operation_id));
        let duplicate_control_plane =
            validated_event("01JZ0000000000000000000002", Some(operation_id));
        let mut tx = pool.begin().await.expect("transaction must begin");

        repository
            .insert(&mut tx, &control_plane, Some(OutboxPairRole::ControlPlane))
            .await
            .expect("control-plane pair member must insert");
        let duplicate = repository
            .insert(
                &mut tx,
                &duplicate_control_plane,
                Some(OutboxPairRole::ControlPlane),
            )
            .await;
        assert!(duplicate.is_err());
        tx.rollback().await.expect("transaction must roll back");
    }

    #[tokio::test]
    async fn audit_outbox_sqlite_pending_rows_are_ordered_by_sequence() {
        let pool = migrated_pool().await;
        let repository = OutboxRepository::new();
        let ids = [
            "01JZ0000000000000000000003",
            "01JZ0000000000000000000001",
            "01JZ0000000000000000000002",
        ];
        let mut tx = pool.begin().await.expect("transaction must begin");
        for id in ids {
            let event = validated_event(id, None);
            repository
                .insert(&mut tx, &event, None)
                .await
                .expect("pending event must insert");
        }
        tx.commit().await.expect("transaction must commit");

        let pending = sqlx::query_scalar::<_, String>(
            "SELECT event_id FROM outbox_events WHERE published_at IS NULL ORDER BY seq",
        )
        .fetch_all(&pool)
        .await
        .expect("pending rows must be readable");
        assert_eq!(pending, ids);
    }

    #[tokio::test]
    async fn audit_outbox_sqlite_admin_queries_filter_count_and_paginate_without_claiming() {
        let pool = migrated_pool().await;
        let repository = OutboxRepository::new();
        let first_id = "01JZ0000000000000000000001";
        let second_id = "01JZ0000000000000000000002";
        let third_id = "01JZ0000000000000000000003";
        let operation_id = "01JZ000000000000000000000X";
        let mut tx = pool.begin().await.expect("transaction must begin");

        repository
            .insert(&mut tx, &validated_event(first_id, None), None)
            .await
            .expect("first event must insert");
        repository
            .insert(&mut tx, &validated_event(second_id, None), None)
            .await
            .expect("second event must insert");
        repository
            .insert(
                &mut tx,
                &validated_event(third_id, Some(operation_id)),
                Some(OutboxPairRole::ControlPlane),
            )
            .await
            .expect("paired event must insert");
        tx.commit().await.expect("transaction must commit");

        sqlx::query(
            "UPDATE outbox_events
             SET published_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE event_id = $1",
        )
        .bind(second_id)
        .execute(&pool)
        .await
        .expect("second event must be marked published");

        let all = repository
            .query(&pool, &OutboxEventFilter::default(), 10, 0)
            .await
            .expect("events must be queryable");
        assert_eq!(
            all.iter()
                .map(|row| row.event_id.as_str())
                .collect::<Vec<_>>(),
            vec![third_id, second_id, first_id]
        );

        let published_filter = OutboxEventFilter {
            published: Some(true),
            ..OutboxEventFilter::default()
        };
        let published = repository
            .query(&pool, &published_filter, 10, 0)
            .await
            .expect("published events must be queryable");
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].event_id, second_id);
        assert_eq!(
            repository
                .count(&pool, &published_filter)
                .await
                .expect("published events must be countable"),
            1
        );

        let paired_filter = OutboxEventFilter {
            operation_id: Some(operation_id.to_string()),
            pair_role: Some("control_plane".to_string()),
            published: Some(false),
            ..OutboxEventFilter::default()
        };
        let paired = repository
            .query(&pool, &paired_filter, 10, 0)
            .await
            .expect("paired pending events must be queryable");
        assert_eq!(paired.len(), 1);
        assert_eq!(paired[0].event_id, third_id);

        let page = repository
            .query(&pool, &OutboxEventFilter::default(), 1, 1)
            .await
            .expect("events must support offset pagination");
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].event_id, second_id);

        let by_id = repository
            .get_by_event_id(&pool, first_id)
            .await
            .expect("event lookup must succeed")
            .expect("event must exist");
        assert_eq!(by_id.event_id, first_id);
        assert_eq!(by_id.published_at, None);
    }

    #[tokio::test]
    async fn audit_outbox_sqlite_rollback_leaves_no_event() {
        let pool = migrated_pool().await;
        let repository = OutboxRepository::new();
        let event = validated_event("01JZ0000000000000000000001", None);
        let mut tx = pool.begin().await.expect("transaction must begin");
        repository
            .insert(&mut tx, &event, None)
            .await
            .expect("event must insert before rollback");
        tx.rollback().await.expect("transaction must roll back");

        let count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM outbox_events WHERE event_id = $1")
                .bind(event.event_id().as_str())
                .fetch_one(&pool)
                .await
                .expect("outbox count must be readable");
        assert_eq!(count, 0);
    }
}

#[cfg(all(test, feature = "postgres"))]
mod postgres_tests {
    use chrono::{TimeZone, Utc};
    use sqlx::Connection;

    use super::*;
    use crate::ent::{
        AuditActor, AuditOperation, AuditResource, AuditScopeSelector, AuditService,
        RawAuditEventBuilder,
    };

    const PRE_OUTBOX_MIGRATION: i64 = 20_260_712_000_000;
    static POSTGRES_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations/postgres");

    fn database_url() -> String {
        dotenvy::dotenv().ok();
        std::env::var("TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .expect("TEST_DATABASE_URL or DATABASE_URL must be set")
    }

    fn validated_event(event_id: &str) -> ValidatedAuditEvent {
        RawAuditEventBuilder::new()
            .event_id(AuditId::parse(event_id).expect("test event id must be valid"))
            .occurred_at(
                Utc.with_ymd_and_hms(2026, 7, 19, 10, 0, 0)
                    .single()
                    .expect("test timestamp must be valid"),
            )
            .scope(AuditScopeSelector::control_plane())
            .actor(AuditActor::new("system"))
            .service(AuditService::stargate())
            .resource(AuditResource::new("user", "user-123"))
            .action("user.updated")
            .operation(AuditOperation::Update)
            .build()
            .expect("test event must validate")
    }

    async fn isolated_connection() -> (sqlx::PgConnection, String) {
        let mut connection = sqlx::PgConnection::connect(&database_url())
            .await
            .expect("PostgreSQL test connection must open");
        let schema = format!(
            "audit_a2_{}",
            ulid::Ulid::generate().to_string().to_ascii_lowercase()
        );
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA \"{schema}\"")))
            .execute(&mut connection)
            .await
            .expect("isolated PostgreSQL schema must be created");
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "SET search_path TO \"{schema}\", public"
        )))
        .execute(&mut connection)
        .await
        .expect("isolated PostgreSQL search path must be selected");
        // The historical init migration checks enum names database-wide rather
        // than per schema. Shadow them locally so the final DROP TYPE can never
        // resolve and remove a type from `public` when this test runs against a
        // shared developer database.
        sqlx::query(
            "CREATE TYPE action_type AS ENUM ('create', 'update', 'delete', 'read', 'login', 'logout')",
        )
        .execute(&mut connection)
        .await
        .expect("isolated action_type must be created");
        sqlx::query(
            "CREATE TYPE actor_type AS ENUM ('admin', 'admin_key', 'user', 'api_key', 'system', 'anonymous')",
        )
        .execute(&mut connection)
        .await
        .expect("isolated actor_type must be created");
        (connection, schema)
    }

    async fn drop_isolated_schema(connection: &mut sqlx::PgConnection, schema: &str) {
        sqlx::query("SET search_path TO public")
            .execute(&mut *connection)
            .await
            .expect("PostgreSQL search path must reset");
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA \"{schema}\" CASCADE"
        )))
        .execute(connection)
        .await
        .expect("isolated PostgreSQL schema must be removed");
    }

    #[tokio::test]
    #[ignore = "requires a live PostgreSQL test database"]
    async fn audit_outbox_postgres_migrates_blank_schema() {
        let (mut connection, schema) = isolated_connection().await;
        POSTGRES_MIGRATOR
            .run_direct(None, &mut connection, false)
            .await
            .expect("PostgreSQL migrations must apply to a blank schema");

        let columns = sqlx::query_scalar::<_, String>(
            r#"
            SELECT column_name::text
            FROM information_schema.columns
            WHERE table_schema = current_schema()
              AND table_name = 'outbox_events'
            ORDER BY ordinal_position
            "#,
        )
        .fetch_all(&mut connection)
        .await
        .expect("PostgreSQL outbox columns must be readable");
        assert_eq!(
            columns,
            vec![
                "event_id",
                "payload",
                "seq",
                "operation_id",
                "pair_role",
                "created_at",
                "published_at",
            ]
        );
        let audits_exist =
            sqlx::query_scalar::<_, bool>("SELECT to_regclass('audits') IS NOT NULL")
                .fetch_one(&mut connection)
                .await
                .expect("PostgreSQL legacy audit lookup must succeed");
        assert!(!audits_exist);
        let legacy_types_exist = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1
                FROM pg_type type
                JOIN pg_namespace namespace ON namespace.oid = type.typnamespace
                WHERE namespace.nspname = current_schema()
                  AND type.typname IN ('action_type', 'actor_type')
            )",
        )
        .fetch_one(&mut connection)
        .await
        .expect("PostgreSQL legacy audit type lookup must succeed");
        assert!(!legacy_types_exist);

        drop_isolated_schema(&mut connection, &schema).await;
    }

    #[tokio::test]
    #[ignore = "requires a live PostgreSQL test database"]
    async fn audit_outbox_postgres_upgrades_pre_refactor_schema_and_drops_legacy_audits() {
        let (mut connection, schema) = isolated_connection().await;
        POSTGRES_MIGRATOR
            .run_direct(Some(PRE_OUTBOX_MIGRATION), &mut connection, false)
            .await
            .expect("pre-outbox PostgreSQL migrations must apply");

        POSTGRES_MIGRATOR
            .run_direct(None, &mut connection, false)
            .await
            .expect("PostgreSQL outbox migration must apply");
        let outbox_exists =
            sqlx::query_scalar::<_, bool>("SELECT to_regclass('outbox_events') IS NOT NULL")
                .fetch_one(&mut connection)
                .await
                .expect("PostgreSQL outbox table lookup must succeed");
        let legacy_audits_exist =
            sqlx::query_scalar::<_, bool>("SELECT to_regclass('audits') IS NOT NULL")
                .fetch_one(&mut connection)
                .await
                .expect("legacy PostgreSQL audit lookup must succeed");
        let legacy_types_exist = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1
                FROM pg_type type
                JOIN pg_namespace namespace ON namespace.oid = type.typnamespace
                WHERE namespace.nspname = current_schema()
                  AND type.typname IN ('action_type', 'actor_type')
            )",
        )
        .fetch_one(&mut connection)
        .await
        .expect("legacy PostgreSQL audit type lookup must succeed");
        assert!(outbox_exists);
        assert!(!legacy_audits_exist);
        assert!(!legacy_types_exist);

        drop_isolated_schema(&mut connection, &schema).await;
    }

    #[tokio::test]
    #[ignore = "requires a live PostgreSQL test database"]
    async fn audit_outbox_postgres_claims_in_order_and_marks_published() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url())
            .await
            .expect("PostgreSQL test pool must connect");
        POSTGRES_MIGRATOR
            .run(&pool)
            .await
            .expect("PostgreSQL migrations must apply");
        let repository = OutboxRepository::new();
        let first_id = AuditId::new().to_string();
        let second_id = AuditId::new().to_string();
        let first = validated_event(&first_id);
        let second = validated_event(&second_id);
        let mut insert_tx = pool.begin().await.expect("insert transaction must begin");
        let first_row = repository
            .insert(&mut insert_tx, &first, None)
            .await
            .expect("first PostgreSQL event must insert");
        let second_row = repository
            .insert(&mut insert_tx, &second, None)
            .await
            .expect("second PostgreSQL event must insert");
        insert_tx
            .commit()
            .await
            .expect("insert transaction must commit");
        assert!(first_row.seq < second_row.seq);

        let mut claim_tx = pool.begin().await.expect("claim transaction must begin");
        let claimed = repository
            .claim_unpublished(&mut claim_tx, 100_000)
            .await
            .expect("PostgreSQL pending events must claim");
        let own_rows = claimed
            .iter()
            .filter(|row| row.event_id == first_id || row.event_id == second_id)
            .collect::<Vec<_>>();
        assert_eq!(own_rows.len(), 2);
        assert!(own_rows[0].seq < own_rows[1].seq);
        let own_seqs = own_rows.iter().map(|row| row.seq).collect::<Vec<_>>();
        repository
            .mark_published(&mut claim_tx, &own_seqs)
            .await
            .expect("claimed PostgreSQL events must mark published");
        claim_tx
            .commit()
            .await
            .expect("claim transaction must commit");

        let published = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM outbox_events WHERE event_id = ANY($1) AND published_at IS NOT NULL",
        )
        .bind(&[first_id.as_str(), second_id.as_str()][..])
        .fetch_one(&pool)
        .await
        .expect("published PostgreSQL rows must be countable");
        assert_eq!(published, 2);

        sqlx::query("DELETE FROM outbox_events WHERE event_id = ANY($1)")
            .bind(&[first_id.as_str(), second_id.as_str()][..])
            .execute(&pool)
            .await
            .expect("PostgreSQL test rows must be removed");
        pool.close().await;
    }
}
