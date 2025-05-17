use crate::db;
use crate::ent::record::thing_to_string;
use serde::{Deserialize, Serialize};

const RESOURCE: &str = "tokens";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    #[serde(deserialize_with = "thing_to_string")]
    pub id: String,
    pub value: String,
}

impl Token {
    pub async fn save(token: Token) -> surrealdb::Result<Option<Token>> {
        let db = db::connection().await?;
        let sql = format!(
            "INSERT INTO {} (id, value) VALUES ($id, $value) ON DUPLICATE KEY UPDATE value = $value",
            RESOURCE
        );
        let mut responde = db
            .query(sql)
            .bind(("id", token.id))
            .bind(("value", token.value))
            .await?;

        let tokens: Vec<Token> = responde.take(0)?;
        let token = tokens.first().cloned();
        Ok(token)
    }

    pub async fn get(id: &str) -> surrealdb::Result<Option<Token>> {
        let db = db::connection().await?;
        db.select((RESOURCE, id)).await
    }

    pub async fn delete(id: &str) -> surrealdb::Result<Option<Token>> {
        let db = db::connection().await?;
        db.delete((RESOURCE, id)).await
    }
}
