use crate::ent::{ConfigDocument, ConfigKind};
use anyhow::Result;

#[derive(Clone, Default)]
pub struct ConfigDocumentRepository {}

impl ConfigDocumentRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn latest<'c, E>(&self, ex: E, kind: ConfigKind) -> Result<Option<ConfigDocument>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        Ok(sqlx::query_as::<_, ConfigDocument>(
            "SELECT * FROM config_documents WHERE kind = $1 ORDER BY version DESC LIMIT 1",
        )
        .bind(kind.as_str())
        .fetch_optional(ex)
        .await?)
    }

    pub async fn latest_revisions<'c, E>(&self, ex: E) -> Result<Vec<(String, i64, String)>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        Ok(sqlx::query_as(
            "SELECT kind, version, revision FROM config_documents
             WHERE (kind, version) IN (SELECT kind, MAX(version) FROM config_documents GROUP BY kind)
             ORDER BY kind",
        )
        .fetch_all(ex)
        .await?)
    }

    pub async fn insert(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        document: &ConfigDocument,
    ) -> Result<ConfigDocument> {
        Ok(sqlx::query_as::<_, ConfigDocument>(
            "INSERT INTO config_documents
                (id, kind, version, revision, content, source, created_by, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING *",
        )
        .bind(&document.id)
        .bind(&document.kind)
        .bind(document.version)
        .bind(&document.revision)
        .bind(&document.content)
        .bind(&document.source)
        .bind(&document.created_by)
        .bind(document.created_at)
        .fetch_one(&mut **tx)
        .await?)
    }
}
