use crate::ent::User;
use anyhow::Result;
use chrono::Utc;

pub const USER: &str = "users";

#[derive(Clone)]
pub struct UserRepository {}

impl UserRepository {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for UserRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl UserRepository {
    pub async fn create(&self, tx: &mut crate::backend::Tx<'_>, user: User) -> Result<User> {
        let row = sqlx::query_as::<_, User>(
            sqlx::AssertSqlSafe(format!(
                "
            INSERT INTO {users} (id, email, given_name, family_name, nickname, picture, phone_number, attrs, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING *
        ",
                users = USER
            )),
        )
        .bind(&user.id)
        .bind(&user.email)
        .bind(&user.given_name)
        .bind(&user.family_name)
        .bind(&user.nickname)
        .bind(&user.picture)
        .bind(&user.phone_number)
        .bind(&user.attrs)
        .bind(user.created_at)
        .bind(user.updated_at)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn search<'c, E>(&self, ex: E, query: &str) -> Result<Vec<User>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, User>(
            sqlx::AssertSqlSafe(format!(
                "
            SELECT * FROM {users}
            WHERE email {like} $1 OR given_name {like} $1 OR family_name {like} $1 OR nickname {like} $1
            ORDER BY created_at DESC, id DESC
        ",
                users = USER,
                like = crate::backend::LIKE
            )),
        )
        .bind(format!("%{}%", query))
        .fetch_all(ex)
        .await?;

        Ok(rows)
    }

    pub async fn get_by_username<'c, E>(&self, ex: E, username: &str) -> Result<Option<User>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row = sqlx::query_as::<_, User>(sqlx::AssertSqlSafe(format!(
            "
            SELECT * FROM {users} WHERE nickname = $1 OR email = $1 OR phone_number = $1
        ",
            users = USER
        )))
        .bind(username)
        .fetch_optional(ex)
        .await?;

        Ok(row)
    }

    pub async fn update(&self, tx: &mut crate::backend::Tx<'_>, user: User) -> Result<User> {
        let updated_at = Utc::now();
        let row = sqlx::query_as::<_, User>(
            sqlx::AssertSqlSafe(format!(
                "
            UPDATE {users}
            SET email = $2, given_name = $3, family_name = $4, nickname = $5, picture = $6, phone_number = $7, attrs = $8, updated_at = $9
            WHERE id = $1
            RETURNING *
        ",
                users = USER
            )),
        )
        .bind(&user.id)
        .bind(&user.email)
        .bind(&user.given_name)
        .bind(&user.family_name)
        .bind(&user.nickname)
        .bind(&user.picture)
        .bind(&user.phone_number)
        .bind(&user.attrs)
        .bind(updated_at)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_all<'c, E>(&self, ex: E, limit: i64, offset: i64) -> Result<Vec<User>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, User>(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM {users} ORDER BY created_at DESC, id DESC LIMIT $1 OFFSET $2",
            users = USER
        )))
        .bind(limit)
        .bind(offset)
        .fetch_all(ex)
        .await?;

        Ok(rows)
    }

    pub async fn count<'c, E>(&self, ex: E) -> Result<i64>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row: (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {users}",
            users = USER
        )))
        .fetch_one(ex)
        .await?;

        Ok(row.0)
    }

    pub async fn search_with_pagination<'c, E>(
        &self,
        ex: E,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<User>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let pattern = format!("%{}%", query);
        let rows = sqlx::query_as::<_, User>(
            sqlx::AssertSqlSafe(format!(
                "SELECT * FROM {users}
                 WHERE email {like} $1 OR given_name {like} $1 OR family_name {like} $1 OR nickname {like} $1
                 ORDER BY created_at DESC, id DESC LIMIT $2 OFFSET $3",
                users = USER,
                like = crate::backend::LIKE
            )),
        )
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(ex)
        .await?;

        Ok(rows)
    }

    pub async fn count_search<'c, E>(&self, ex: E, query: &str) -> Result<i64>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let pattern = format!("%{}%", query);
        let row: (i64,) = sqlx::query_as(
            sqlx::AssertSqlSafe(format!(
                "SELECT COUNT(*) FROM {users}
                 WHERE email {like} $1 OR given_name {like} $1 OR family_name {like} $1 OR nickname {like} $1",
                users = USER,
                like = crate::backend::LIKE
            )),
        )
        .bind(&pattern)
        .fetch_one(ex)
        .await?;

        Ok(row.0)
    }

    pub async fn get_by_id<'c, E>(&self, ex: E, id: &str) -> Result<Option<User>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row = sqlx::query_as::<_, User>(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM {users} WHERE id = $1",
            users = USER
        )))
        .bind(id)
        .fetch_optional(ex)
        .await?;

        Ok(row)
    }

    pub async fn delete(&self, tx: &mut crate::backend::Tx<'_>, id: &str) -> Result<()> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {users} WHERE id = $1",
            users = USER
        )))
        .bind(id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}
