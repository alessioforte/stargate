use serde::{Deserialize, Serialize};

use crate::db::DB;
use crate::models::records::thing_to_string;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    #[serde(deserialize_with = "thing_to_string")]
    pub id: String,
    pub value: String,
    pub expires_at: i64,
    pub issued_at: i64,
}

impl Token {
    pub async fn upsert(token: Token) -> surrealdb::Result<Option<Token>> {
        let sql = r#"
            INSERT INTO tokens (id, value, expires_at, issued_at)
            VALUES ($id, $value, $expires_at, $issued_at)
            ON DUPLICATE KEY UPDATE
            value = $input.value,
            expires_at = $input.expires_at,
            issued_at = $input.issued_at
        "#;
        let mut responde = DB
            .query(sql)
            .bind(("id", token.id))
            .bind(("value", token.value))
            .bind(("expires_at", token.expires_at))
            .bind(("issued_at", token.issued_at))
            .await?;

        let tokens: Vec<Token> = responde.take(0)?;
        let token = tokens.first().cloned();
        Ok(token)
    }

    pub async fn get(id: String) -> surrealdb::Result<Option<Token>> {
        DB.select(("tokens", id)).await
    }
}
