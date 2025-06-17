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
        tx: &mut sqlx::Transaction<'_, sqlx::Any>,
        user: User,
    ) -> Result<User> {
        let row = sqlx::query_as::<_, User>(
            "
            INSERT INTO users (id, email, first_name, last_name, nickname, picture, phone_number)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
        ",
        )
        .bind(&user.id)
        .bind(&user.email)
        .bind(&user.first_name)
        .bind(&user.last_name)
        .bind(&user.nickname)
        .bind(&user.picture)
        .bind(&user.phone_number)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_by_username(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Any>,
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
        tx: &mut sqlx::Transaction<'_, sqlx::Any>,
        user: User,
    ) -> Result<User> {
        let row = sqlx::query_as::<_, User>(
            "
            UPDATE users
            SET email = $2, first_name = $3, last_name = $4, nickname = $5, picture = $6, phone_number = $7
            WHERE id = $1
            RETURNING *
        ",
        )
        .bind(&user.id)
        .bind(&user.email)
        .bind(&user.first_name)
        .bind(&user.last_name)
        .bind(&user.nickname)
        .bind(&user.picture)
        .bind(&user.phone_number)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }
}
