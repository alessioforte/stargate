use crate::ent::{Subject, SubjectType};
use anyhow::Result;
use sqlx::types::JsonValue;

#[derive(Clone)]
pub struct SubjectRepository {}

impl SubjectRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn create(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        sub_type: SubjectType,
        sub_id: &str,
        attrs: Option<JsonValue>,
    ) -> Result<Subject> {
        let subject = Subject::new(sub_type, sub_id.to_string(), attrs);
        let row = sqlx::query_as::<_, Subject>(
            "
            INSERT INTO subjects (id, type, sub_id, attrs)
            VALUES ($1, $2, $3, $4)
            RETURNING *
        ",
        )
        .bind(&subject.id)
        .bind(&subject.sub_type)
        .bind(&subject.sub_id)
        .bind(&subject.attrs)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_by_id(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
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
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
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
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: &str,
        attrs: JsonValue,
    ) -> Result<Subject> {
        let row = sqlx::query_as::<_, Subject>(
            "
            UPDATE subjects SET attrs = $1 WHERE id = $2 RETURNING *
        ",
        )
        .bind(attrs)
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }
}
