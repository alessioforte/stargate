use crate::ent::{Account, AccountType};
use anyhow::Result;

pub const ACCOUNT: &str = "accounts";

#[derive(Clone)]
pub struct AccountRepository {}

impl AccountRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn create(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        account_type: AccountType,
        name: &str,
        description: Option<&str>,
    ) -> Result<Account> {
        let account = Account::new(
            name.to_string(),
            account_type,
            description.map(|d| d.to_string()),
        );

        let row = sqlx::query_as::<_, Account>(
            format!(
                "
            INSERT INTO {accounts} (id, type, name, description)
            VALUES ($1, $2, $3, $4)
            RETURNING *
        ",
                accounts = ACCOUNT
            )
            .as_str(),
        )
        .bind(&account.id)
        .bind(&account.account_type)
        .bind(&account.name)
        .bind(&account.description)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_by_id(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: &str,
    ) -> Result<Option<Account>> {
        let row = sqlx::query_as::<_, Account>(
            format!(
                "
            SELECT * FROM {accounts} WHERE id = $1
        ",
                accounts = ACCOUNT
            )
            .as_str(),
        )
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;

        Ok(row)
    }
}
