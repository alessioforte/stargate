use crate::ent::User;
use anyhow::Result;

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
            "
            INSERT INTO users (id, account_id, email, given_name, family_name, nickname, picture, phone_number, attrs)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING *
        ",
        )
        .bind(&user.id)
        .bind(&user.account_id)
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
            "
            SELECT * FROM users
            WHERE email ILIKE $1 OR given_name ILIKE $1 OR family_name ILIKE $1 OR nickname ILIKE $1
            ORDER BY created_at DESC
        ",
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
            "
            SELECT * FROM users WHERE nickname = $1 OR email = $1 OR phone_number = $1
        ",
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
            "
            UPDATE users
            SET email = $2, given_name = $3, family_name = $4, nickname = $5, picture = $6, phone_number = $7
            WHERE id = $1
            RETURNING *
        ",
        )
        .bind(&user.id)
        .bind(&user.email)
        .bind(&user.given_name)
        .bind(&user.family_name)
        .bind(&user.nickname)
        .bind(&user.picture)
        .bind(&user.phone_number)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }
}
