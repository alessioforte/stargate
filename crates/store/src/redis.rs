use crate::store::Store;
use async_trait::async_trait;
use redis::Commands;

pub struct RedisStore {
    client: redis::Client,
}

impl RedisStore {
    pub fn new(url: &str) -> redis::RedisResult<Self> {
        let client = redis::Client::open(url)?;
        Ok(Self { client })
    }
}

#[async_trait]
impl Store for RedisStore {
    async fn get<T: serde::de::DeserializeOwned + Send + Sync>(&self, key: &str) -> Option<T> {
        let mut con = self.client.get_connection().unwrap();
        let value: String = con.get(key).unwrap();
        serde_json::from_str(&value).ok()
    }

    async fn set<T: serde::Serialize + Send + Sync>(&self, key: &str, value: &T, ttl: Option<u64>) {
        let mut con = self.client.get_connection().unwrap();
        let serialized_value = serde_json::to_string(value).expect("Failed to serialize value");
        if let Some(ttl) = ttl {
            con.set_ex(key, serialized_value, ttl).unwrap()
        } else {
            let _: () = con.set(key, serialized_value).unwrap();
        }
    }

    async fn delete(&self, key: &str) -> bool {
        let mut con = self.client.get_connection().unwrap();
        con.del(key).unwrap()
    }
}
