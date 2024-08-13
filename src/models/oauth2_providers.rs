use crate::db::DB;
use crate::model;
use crate::models::record::thing_to_string;
use crate::models::record::Record;
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
    }
}

impl Oauth2Provider {
    pub async fn save(provider: Oauth2Provider) -> surrealdb::Result<Vec<Record>> {
        let db = DB.get().unwrap();
        db.create(RESOURCE).content(provider).await
    }

    pub async fn get_by_provider(
        provider: &str,
        provider_user_id: &str,
    ) -> surrealdb::Result<Option<Oauth2Provider>> {
        let db = DB.get().unwrap();
        let id = format!("{}:{}", provider, provider_user_id);
        db.select((RESOURCE, id)).await
    }
}
