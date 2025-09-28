use crate::ent::{Limits, Subject, SubjectType};
use anyhow::Result;
use serde_json::Value as JsonValue;

#[derive(Clone)]
pub struct SubjectRepository {}

impl SubjectRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn create(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        sub_type: SubjectType,
        sub_id: &str,
        attrs: Option<JsonValue>,
        limits: Option<Limits>,
    ) -> Result<Subject> {
        let subject = Subject::new(sub_type, sub_id.to_string(), attrs, limits);
        let row = sqlx::query_as::<_, Subject>(
            "
            INSERT INTO subjects (id, type, sub_id, attrs, limits)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING *
        ",
        )
        .bind(&subject.id)
        .bind(&subject.sub_type)
        .bind(&subject.sub_id)
        .bind(&subject.attrs)
        .bind(&subject.limits)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_by_id(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: &str,
    ) -> Result<Option<Subject>> {
        let row = sqlx::query_as::<_, Subject>(
            "
            SELECT * FROM subjects WHERE id = $1
        ",
        )
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_by_sub_id(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        sub_id: &str,
    ) -> Result<Option<Subject>> {
        let row = sqlx::query_as::<_, Subject>(
            "
            SELECT * FROM subjects WHERE sub_id = $1
        ",
        )
        .bind(sub_id)
        .fetch_optional(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn update_attrs(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        sub_id: &str,
        attrs: JsonValue,
    ) -> Result<Subject> {
        let attrs_str = serde_json::to_string(&attrs)?;
        let row = sqlx::query_as::<_, Subject>(
            "
            UPDATE subjects SET attrs = $1 WHERE sub_id = $2 RETURNING *
        ",
        )
        .bind(attrs_str)
        .bind(sub_id)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn update_limits(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        sub_id: &str,
        limits: Limits,
    ) -> Result<Subject> {
        let limits_str = serde_json::to_string(&limits)?;
        let row = sqlx::query_as::<_, Subject>(
            "
            UPDATE subjects SET limits = $1 WHERE sub_id = $2 RETURNING *
        ",
        )
        .bind(limits_str)
        .bind(sub_id)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }
}
