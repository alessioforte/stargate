use crate::db::DB;
use crate::model;
use crate::models::record::Record;
use serde::{Deserialize, Serialize};

const RESOURCE: &str = "oauth2_providers";

model! {
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Oauth2Provider {
        pub user_id: String,
        pub provider: String,
        pub provider_id: String,
    }
}

impl Oauth2Provider {
    pub async fn create(provider: Oauth2Provider) -> surrealdb::Result<Vec<Record>> {
        let db = DB.get().unwrap();
        db.create(RESOURCE).content(provider).await
    }

    pub async fn get_by_provider_id(
        provider_id: String,
    ) -> surrealdb::Result<Option<Oauth2Provider>> {
        let db = DB.get().unwrap();
        db.select((RESOURCE, provider_id)).await
    }
}
