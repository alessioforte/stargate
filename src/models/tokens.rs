use serde::{Deserialize, Serialize};

use crate::db::DB;
use crate::models::records::thing_to_string;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    #[serde(deserialize_with = "thing_to_string")]
    pub id: String,
    pub value: String,
}

impl Token {
    pub async fn upsert(token: Token) -> surrealdb::Result<Option<Token>> {
        let db = DB.get().unwrap();
        let sql = r#"
            INSERT INTO tokens (id, value)
            VALUES ($id, $value)
            ON DUPLICATE KEY UPDATE
            value = $input.value
        "#;
        let mut responde = db
            .query(sql)
            .bind(("id", token.id))
            .bind(("value", token.value))
            .await?;

        let tokens: Vec<Token> = responde.take(0)?;
        let token = tokens.first().cloned();
        Ok(token)
    }

    pub async fn get(id: String) -> surrealdb::Result<Option<Token>> {
        let db = DB.get().unwrap();
        db.select(("tokens", id)).await
    }

    pub async fn delete(id: String) -> surrealdb::Result<Option<Token>> {
        let db = DB.get().unwrap();
        db.delete(("tokens", id)).await
    }
}
