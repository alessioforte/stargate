use crate::ent::{Organization, User};
use anyhow::Result;

pub const ORGANIZATION: &str = "organizations";
pub const USER_ORGANIZATION: &str = "user_organizations";

#[cfg(feature = "sqlite")]
const LIKE: &str = "LIKE";
#[cfg(feature = "postgres")]
const LIKE: &str = "ILIKE";

#[derive(Clone)]
pub struct OrganizationRepository {}

impl OrganizationRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn create(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        name: &str,
        description: Option<&str>,
        attrs: Option<&serde_json::Value>,
    ) -> Result<Organization> {
        let org = Organization::new(name.to_string(), description.map(|d| d.to_string()));

        let row = sqlx::query_as::<_, Organization>(
            format!(
                "
            INSERT INTO {tbl} (id, name, description, attrs)
            VALUES ($1, $2, $3, $4)
            RETURNING *
        ",
                tbl = ORGANIZATION
            )
            .as_str(),
        )
        .bind(&org.id)
        .bind(&org.name)
        .bind(&org.description)
        .bind(&attrs)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_by_id(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: &str,
    ) -> Result<Option<Organization>> {
        let row = sqlx::query_as::<_, Organization>(
            format!("SELECT * FROM {tbl} WHERE id = $1", tbl = ORGANIZATION).as_str(),
        )
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_all(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Organization>> {
        let rows = sqlx::query_as::<_, Organization>(
            format!(
                "SELECT * FROM {tbl} ORDER BY id DESC LIMIT $1 OFFSET $2",
                tbl = ORGANIZATION
            )
            .as_str(),
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut **tx)
        .await?;

        Ok(rows)
    }

    pub async fn count(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<i64> {
        let row: (i64,) =
            sqlx::query_as(format!("SELECT COUNT(*) FROM {tbl}", tbl = ORGANIZATION).as_str())
                .fetch_one(&mut **tx)
                .await?;

        Ok(row.0)
    }

    pub async fn search(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Organization>> {
        let pattern = format!("%{}%", query);
        let rows = sqlx::query_as::<_, Organization>(
            format!(
                "
            SELECT * FROM {tbl}
            WHERE name {like} $1 OR description {like} $1
            ORDER BY id DESC LIMIT $2 OFFSET $3
        ",
                tbl = ORGANIZATION,
                like = LIKE
            )
            .as_str(),
        )
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut **tx)
        .await?;

        Ok(rows)
    }

    pub async fn count_search(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        query: &str,
    ) -> Result<i64> {
        let pattern = format!("%{}%", query);
        let row: (i64,) = sqlx::query_as(
            format!(
                "
            SELECT COUNT(*) FROM {tbl}
            WHERE name {like} $1 OR description {like} $1
        ",
                tbl = ORGANIZATION,
                like = LIKE
            )
            .as_str(),
        )
        .bind(&pattern)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row.0)
    }

    pub async fn update(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: &str,
        name: &str,
        description: Option<&str>,
        attrs: Option<&serde_json::Value>,
    ) -> Result<Organization> {
        let row = sqlx::query_as::<_, Organization>(
            format!(
                "
            UPDATE {tbl} SET name = $2, description = $3, attrs = $4 WHERE id = $1
            RETURNING *
        ",
                tbl = ORGANIZATION
            )
            .as_str(),
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(attrs)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn delete(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: &str,
    ) -> Result<()> {
        sqlx::query(format!("DELETE FROM {tbl} WHERE id = $1", tbl = ORGANIZATION).as_str())
            .bind(id)
            .execute(&mut **tx)
            .await?;

        Ok(())
    }

    pub async fn add_user(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: &str,
        org_id: &str,
    ) -> Result<()> {
        sqlx::query(
            format!(
                "INSERT INTO {tbl} (user_id, org_id) VALUES ($1, $2)",
                tbl = USER_ORGANIZATION
            )
            .as_str(),
        )
        .bind(user_id)
        .bind(org_id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub async fn remove_user(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: &str,
        org_id: &str,
    ) -> Result<()> {
        sqlx::query(
            format!(
                "DELETE FROM {tbl} WHERE user_id = $1 AND org_id = $2",
                tbl = USER_ORGANIZATION
            )
            .as_str(),
        )
        .bind(user_id)
        .bind(org_id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub async fn get_users(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        org_id: &str,
    ) -> Result<Vec<User>> {
        let rows = sqlx::query_as::<_, User>(
            format!(
                "
            SELECT u.* FROM users u
            INNER JOIN {tbl} uo ON u.id = uo.user_id
            WHERE uo.org_id = $1
            ORDER BY u.id
        ",
                tbl = USER_ORGANIZATION
            )
            .as_str(),
        )
        .bind(org_id)
        .fetch_all(&mut **tx)
        .await?;

        Ok(rows)
    }

    pub async fn get_orgs_by_user(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: &str,
    ) -> Result<Vec<Organization>> {
        let rows = sqlx::query_as::<_, Organization>(
            format!(
                "
            SELECT o.* FROM {tbl_org} o
            INNER JOIN {tbl_uo} uo ON o.id = uo.org_id
            WHERE uo.user_id = $1
            ORDER BY o.id
        ",
                tbl_org = ORGANIZATION,
                tbl_uo = USER_ORGANIZATION
            )
            .as_str(),
        )
        .bind(user_id)
        .fetch_all(&mut **tx)
        .await?;

        Ok(rows)
    }
}
