use crate::ent::{ConfigDocument, ConfigKind, ConfigRevision, ConfigVersionSummary};
use anyhow::Result;

#[cfg(feature = "postgres")]
const CONTENT_SIZE_BYTES: &str = "OCTET_LENGTH(content)::bigint";
#[cfg(feature = "sqlite")]
const CONTENT_SIZE_BYTES: &str = "LENGTH(CAST(content AS BLOB))";

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

    pub async fn list<'c, E>(
        &self,
        ex: E,
        kind: ConfigKind,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ConfigVersionSummary>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let mut query = sqlx::QueryBuilder::<crate::backend::Db>::new(
            "SELECT version, revision, source, created_by, created_at, ",
        );
        query
            .push(CONTENT_SIZE_BYTES)
            .push(" AS size_bytes FROM config_documents WHERE kind = ")
            .push_bind(kind.as_str())
            .push(" ORDER BY version DESC LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
        Ok(query
            .build_query_as::<ConfigVersionSummary>()
            .fetch_all(ex)
            .await?)
    }

    pub async fn get<'c, E>(
        &self,
        ex: E,
        kind: ConfigKind,
        version: i64,
    ) -> Result<Option<ConfigDocument>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        Ok(sqlx::query_as::<_, ConfigDocument>(
            "SELECT * FROM config_documents WHERE kind = $1 AND version = $2",
        )
        .bind(kind.as_str())
        .bind(version)
        .fetch_optional(ex)
        .await?)
    }

    pub async fn latest_revisions<'c, E>(&self, ex: E) -> Result<Vec<ConfigRevision>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT kind, revision FROM config_documents
             WHERE (kind, version) IN (SELECT kind, MAX(version) FROM config_documents GROUP BY kind)
             ORDER BY kind",
        )
        .fetch_all(ex)
        .await?;
        rows.into_iter()
            .map(|(kind, revision)| parse_revision(&kind, revision))
            .collect()
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

fn parse_revision(kind: &str, revision: String) -> Result<ConfigRevision> {
    let kind = ConfigKind::parse(kind)
        .ok_or_else(|| anyhow::anyhow!("unknown configuration kind: {kind}"))?;
    Ok(ConfigRevision { kind, revision })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn revision_kinds_are_typed_and_unknown_database_kinds_are_errors() {
        assert_eq!(
            parse_revision("gateway", "r1".into()).unwrap().kind,
            ConfigKind::Gateway
        );
        assert_eq!(
            parse_revision("policies", "r2".into()).unwrap().kind,
            ConfigKind::Policies
        );
        assert!(parse_revision("unknown", "r3".into()).is_err());
    }
}

#[cfg(test)]
mod history_tests;
