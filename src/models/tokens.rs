use crate::db::DB;
use crate::models::records::thing_to_string;
use serde::{Deserialize, Serialize};

const RESOURCE: &str = "tokens";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    #[serde(deserialize_with = "thing_to_string")]
    pub id: String,
    pub value: String,
}

impl Token {
    pub async fn upsert(token: Token) -> surrealdb::Result<Option<Token>> {
        let db = DB.get().unwrap();
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

    pub async fn get(id: String) -> surrealdb::Result<Option<Token>> {
        let db = DB.get().unwrap();
        db.select((RESOURCE, id)).await
    }

    pub async fn delete(id: String) -> surrealdb::Result<Option<Token>> {
        let db = DB.get().unwrap();
        db.delete((RESOURCE, id)).await
    }
}
