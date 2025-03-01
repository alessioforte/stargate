use crate::db;
use crate::ent::record::thing_to_string;
use crate::ent::record::Record;
use crate::model;
use serde::{Deserialize, Serialize};

const RESOURCE: &str = "oauth2_providers";

model! {
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Oauth2Provider {
        #[serde(deserialize_with = "thing_to_string")]
        pub id: String,
        pub user_id: String,
        // pub access_count: i32,
        // pub last_access: i64,
    }
}

impl Oauth2Provider {
    pub async fn save(provider: Oauth2Provider) -> surrealdb::Result<Option<Record>> {
        let db = db::connection().await?;
        db.create(RESOURCE).content(provider).await
    }

    pub async fn get_by_provider(
        provider: &str,
        provider_user_id: &str,
    ) -> surrealdb::Result<Option<Oauth2Provider>> {
        let db = db::connection().await?;
        let id = format!("{}:{}", provider, provider_user_id);
        db.select((RESOURCE, id)).await
    }
}
