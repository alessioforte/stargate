use crate::ent::User;
use anyhow::Result;

pub const USER: &str = "users";

#[cfg(feature = "sqlite")]
const LIKE: &str = "LIKE";
#[cfg(feature = "postgres")]
const LIKE: &str = "ILIKE";

#[derive(Clone)]
pub struct UserRepository {}

impl UserRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn create(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user: User,
    ) -> Result<User> {
        let row = sqlx::query_as::<_, User>(
            format!(
                "
            INSERT INTO {users} (id, email, given_name, family_name, nickname, picture, phone_number, attrs)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *
        ",
                users = USER
            )
            .as_str(),
        )
        .bind(&user.id)
        .bind(&user.email)
        .bind(&user.given_name)
        .bind(&user.family_name)
        .bind(&user.nickname)
        .bind(&user.picture)
        .bind(&user.phone_number)
        .bind(&user.attrs)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn search(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        query: &str,
    ) -> Result<Vec<User>> {
        let rows = sqlx::query_as::<_, User>(
            format!(
                "
            SELECT * FROM {users}
            WHERE email {like} $1 OR given_name {like} $1 OR family_name {like} $1 OR nickname {like} $1
            ORDER BY id DESC
        ",
                users = USER,
                like = LIKE
            )
            .as_str(),
        )
        .bind(format!("%{}%", query))
        .fetch_all(&mut **tx)
        .await?;

        Ok(rows)
    }

    pub async fn get_by_username(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        username: &str,
    ) -> Result<Option<User>> {
        let row = sqlx::query_as::<_, User>(
            format!(
                "
            SELECT * FROM {users} WHERE nickname = $1 OR email = $1 OR phone_number = $1
        ",
                users = USER
            )
            .as_str(),
        )
        .bind(username)
        .fetch_optional(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn update(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user: User,
    ) -> Result<User> {
        let row = sqlx::query_as::<_, User>(
            format!(
                "
            UPDATE {users}
            SET email = $2, given_name = $3, family_name = $4, nickname = $5, picture = $6, phone_number = $7, attrs = $8
            WHERE id = $1
            RETURNING *
        ",
                users = USER
            )
            .as_str(),
        )
        .bind(&user.id)
        .bind(&user.email)
        .bind(&user.given_name)
        .bind(&user.family_name)
        .bind(&user.nickname)
        .bind(&user.picture)
        .bind(&user.phone_number)
        .bind(&user.attrs)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_all(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<User>> {
        let rows = sqlx::query_as::<_, User>(
            format!(
                "SELECT * FROM {users} ORDER BY id LIMIT $1 OFFSET $2",
                users = USER
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
            sqlx::query_as(format!("SELECT COUNT(*) FROM {users}", users = USER).as_str())
                .fetch_one(&mut **tx)
                .await?;

        Ok(row.0)
    }

    pub async fn search_with_pagination(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<User>> {
        let pattern = format!("%{}%", query);
        let rows = sqlx::query_as::<_, User>(
            format!(
                "SELECT * FROM {users}
                 WHERE email {like} $1 OR given_name {like} $1 OR family_name {like} $1 OR nickname {like} $1
                 ORDER BY id LIMIT $2 OFFSET $3",
                users = USER,
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
                "SELECT COUNT(*) FROM {users}
                 WHERE email {like} $1 OR given_name {like} $1 OR family_name {like} $1 OR nickname {like} $1",
                users = USER,
                like = LIKE
            )
            .as_str(),
        )
        .bind(&pattern)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row.0)
    }

    pub async fn get_by_id(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: &str,
    ) -> Result<Option<User>> {
        let row = sqlx::query_as::<_, User>(
            format!("SELECT * FROM {users} WHERE id = $1", users = USER).as_str(),
        )
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn delete(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: &str,
    ) -> Result<()> {
        sqlx::query(format!("DELETE FROM {users} WHERE id = $1", users = USER).as_str())
            .bind(id)
            .execute(&mut **tx)
            .await?;

        Ok(())
    }
}
