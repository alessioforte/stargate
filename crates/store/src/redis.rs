use crate::store::Store;
use async_trait::async_trait;
use redis::{Commands, HashFieldExpirationOptions, SetExpiry};
use std::collections::HashMap;

pub struct RedisStore {
    client: redis::Client,
}

impl Clone for RedisStore {
    fn clone(&self) -> Self {
        RedisStore {
            client: self.client.clone(),
        }
    }
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
        let value: String = match con.get(key) {
            Ok(v) => v,
            Err(e) => {
                println!("Failed to get key {}: {}", key, e);
                return None;
            }
        };
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

    async fn exists(&self, key: &str) -> bool {
        let mut con = self.client.get_connection().unwrap();
        con.exists(key).unwrap()
    }

    async fn hset<T: serde::Serialize + Send + Sync>(
        &self,
        key: &str,
        field: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> bool {
        let mut con = self.client.get_connection().unwrap();
        let serialized_value = serde_json::to_string(value).expect("Failed to serialize value");

        let hash_field_expiration_options = if let Some(ttl) = ttl {
            HashFieldExpirationOptions::default().set_expiration(SetExpiry::EX(ttl))
        } else {
            HashFieldExpirationOptions::default()
        };
        let fields_values = vec![(field, &serialized_value)];
        let result: i32 = con
            .hset_ex(key, &hash_field_expiration_options, &fields_values)
            .unwrap();

        result == 1
    }

    async fn hget<T: serde::de::DeserializeOwned + Send + Sync>(
        &self,
        key: &str,
        field: &str,
    ) -> Option<T> {
        let mut con = self.client.get_connection().unwrap();
        let value: Option<String> = con.hget(key, field).ok()?;
        value.and_then(|v| serde_json::from_str(&v).ok())
    }

    async fn hdel(&self, key: &str, field: &str) -> bool {
        let mut con = self.client.get_connection().unwrap();
        let result: i32 = con.hdel(key, field).unwrap();
        result > 0
    }

    async fn hgetall<T: serde::de::DeserializeOwned + Send + Sync>(
        &self,
        key: &str,
    ) -> HashMap<String, T> {
        let mut con = self.client.get_connection().unwrap();
        let hash_data: HashMap<String, String> = con.hgetall(key).unwrap_or_default();
        let mut result = HashMap::new();

        for (field, value) in hash_data {
            if let Ok(deserialized) = serde_json::from_str::<T>(&value) {
                result.insert(field, deserialized);
            }
        }

        result
    }

    async fn hexists(&self, key: &str, field: &str) -> bool {
        let mut con = self.client.get_connection().unwrap();
        con.hexists(key, field).unwrap_or(false)
    }

    async fn hkeys(&self, key: &str) -> Vec<String> {
        let mut con = self.client.get_connection().unwrap();
        con.hkeys(key).unwrap_or_default()
    }

    async fn hvals<T: serde::de::DeserializeOwned + Send + Sync>(&self, key: &str) -> Vec<T> {
        let mut con = self.client.get_connection().unwrap();
        let values: Vec<String> = con.hvals(key).unwrap_or_default();
        let mut result = Vec::new();

        for value in values {
            if let Ok(deserialized) = serde_json::from_str::<T>(&value) {
                result.push(deserialized);
            }
        }

        result
    }

    async fn hlen(&self, key: &str) -> usize {
        let mut con = self.client.get_connection().unwrap();
        let len: i32 = con.hlen(key).unwrap_or(0);
        len as usize
    }
}
