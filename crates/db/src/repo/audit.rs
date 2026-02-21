use crate::ent::Audit;
use anyhow::Result;

pub const AUDIT: &str = "audits";

#[derive(Clone)]
pub struct AuditRepository {}

impl AuditRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn insert(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        log: &Audit,
    ) -> Result<Audit> {
        let row = sqlx::query_as::<_, Audit>(
            format!(
                "
            INSERT INTO {audits} (id, timestamp, actor_type, actor_id, action, resource, resource_id, request_id, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING *
        ",
                audits = AUDIT
            )
            .as_str(),
        )
        .bind(&log.id)
        .bind(&log.timestamp)
        .bind(&log.actor_type)
        .bind(&log.actor_id)
        .bind(&log.action)
        .bind(&log.resource)
        .bind(&log.resource_id)
        .bind(&log.request_id)
        .bind(&log.metadata)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }
}
