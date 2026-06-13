use crate::ent::Audit;
use anyhow::Result;
use sqlx::QueryBuilder;

pub const AUDIT: &str = "audits";
const AUDIT_INSERT_COLUMNS: usize = 9;

#[derive(Clone)]
pub struct AuditRepository {}

impl AuditRepository {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for AuditRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditRepository {
    pub async fn insert(&self, tx: &mut crate::backend::Tx<'_>, log: &Audit) -> Result<Audit> {
        let row = sqlx::query_as::<_, Audit>(
            sqlx::AssertSqlSafe(format!(
                "
            INSERT INTO {audits} (id, timestamp, actor_type, actor_id, action, resource, resource_id, request_id, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING *
        ",
                audits = AUDIT
            )),
        )
        .bind(&log.id)
        .bind(log.timestamp)
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

    pub async fn insert_bulk(&self, tx: &mut crate::backend::Tx<'_>, logs: &[Audit]) -> Result<()> {
        if logs.is_empty() {
            return Ok(());
        }

        let chunk_size = bulk_insert_chunk_size();
        for chunk in logs.chunks(chunk_size) {
            let mut builder = QueryBuilder::<crate::backend::Db>::new(format!(
                "
            INSERT INTO {audits} (id, timestamp, actor_type, actor_id, action, resource, resource_id, request_id, metadata)
            ",
                audits = AUDIT
            ));

            builder.push_values(chunk.iter(), |mut row, log| {
                row.push_bind(&log.id)
                    .push_bind(log.timestamp)
                    .push_bind(&log.actor_type)
                    .push_bind(&log.actor_id)
                    .push_bind(&log.action)
                    .push_bind(&log.resource)
                    .push_bind(&log.resource_id)
                    .push_bind(&log.request_id)
                    .push_bind(&log.metadata);
            });

            builder.build().execute(&mut **tx).await?;
        }

        Ok(())
    }
}

#[cfg(feature = "postgres")]
impl AuditRepository {
    /// Claim a batch of unpublished outbox rows for the relay, ordered by `seq`.
    ///
    /// `FOR UPDATE SKIP LOCKED` makes the claim cooperative: rows locked by
    /// another relay's open transaction are skipped, so multiple relay nodes can
    /// drain the backlog concurrently without ever claiming the same row. The
    /// caller must keep `tx` open until the rows are shipped, then commit it.
    pub async fn claim_unpublished(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        limit: i64,
    ) -> Result<Vec<crate::ent::OutboxAudit>> {
        let rows = sqlx::query_as::<_, crate::ent::OutboxAudit>(
            "
            SELECT id, timestamp, actor_type, actor_id, action, resource, resource_id, request_id, metadata, seq
            FROM audits
            WHERE published_at IS NULL
            ORDER BY seq
            LIMIT $1
            FOR UPDATE SKIP LOCKED
            ",
        )
        .bind(limit)
        .fetch_all(&mut **tx)
        .await?;

        Ok(rows)
    }

    /// Mark the given outbox rows as published. No-op for an empty slice.
    pub async fn mark_published(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        seqs: &[i64],
    ) -> Result<()> {
        if seqs.is_empty() {
            return Ok(());
        }

        sqlx::query("UPDATE audits SET published_at = NOW() WHERE seq = ANY($1)")
            .bind(seqs)
            .execute(&mut **tx)
            .await?;

        Ok(())
    }
}

fn bulk_insert_chunk_size() -> usize {
    (crate::backend::MAX_BIND_PARAMETERS / AUDIT_INSERT_COLUMNS).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulk_insert_chunk_size_respects_backend_bind_limit() {
        let chunk_size = bulk_insert_chunk_size();

        assert!(chunk_size > 0);
        assert!(chunk_size * AUDIT_INSERT_COLUMNS <= crate::backend::MAX_BIND_PARAMETERS);
        assert!(
            chunk_size == 1
                || (chunk_size + 1) * AUDIT_INSERT_COLUMNS > crate::backend::MAX_BIND_PARAMETERS
        );
    }
}
