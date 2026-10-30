use crate::ent::{ConfigDocument, ConfigKind, ConfigRevision};
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
    let kind = match kind {
        "gateway" => ConfigKind::Gateway,
        "policies" => ConfigKind::Policies,
        _ => anyhow::bail!("unknown configuration kind: {kind}"),
    };
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
