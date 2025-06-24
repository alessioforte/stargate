use crate::ent::{Action, ActionType};
use anyhow::Result;

#[derive(Clone)]
pub struct ActionRepository {}

impl ActionRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn create(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        action: Action,
    ) -> Result<Action> {
        let row = sqlx::query_as::<_, Action>(
            "
            INSERT INTO actions (id, type, sub, value, iat, exp)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
        ",
        )
        .bind(&action.id)
        .bind(&action.action_type)
        .bind(&action.sub)
        .bind(&action.value)
        .bind(&action.iat)
        .bind(&action.exp)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_by_value(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        value: &str,
    ) -> Result<Option<Action>> {
        let row = sqlx::query_as::<_, Action>(
            "
            SELECT * FROM actions WHERE value = $1
        ",
        )
        .bind(value)
        .fetch_optional(&mut **tx)
        .await?;

        Ok(row)
    }

    // pub async fn delete_by_value(
    //     &self,
    //     tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    //     value: &str,
    // ) -> Result<()> {
    //     sqlx::query(
    //         "
    //         DELETE FROM actions WHERE value = $1
    //     ",
    //     )
    //     .bind(value)
    //     .execute(&mut **tx)
    //     .await?;

    //     Ok(())
    // }

    pub async fn get_by_sub_and_type(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        sub: &str,
        action_type: ActionType,
    ) -> Result<Option<Action>> {
        let action_type = action_type.as_str();
        let row = sqlx::query_as::<_, Action>(
            "
            SELECT * FROM actions WHERE sub = $1 AND type = $2
        ",
        )
        .bind(sub)
        .bind(action_type)
        .fetch_optional(&mut **tx)
        .await?;

        Ok(row)
    }
}
