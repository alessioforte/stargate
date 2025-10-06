use crate::error::{StoreError, StoreResult};
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
    pub fn new(url: &str) -> StoreResult<Self> {
        if url.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "Redis URL cannot be empty".to_string(),
            ));
        }

        let client = redis::Client::open(url).map_err(|e| {
            StoreError::ConnectionFailed(format!("Failed to create Redis client: {}", e))
        })?;

        println!("Connected to Redis at {}", url);
        Ok(Self { client })
    }

    /// Get a Redis connection with proper error handling
    async fn get_connection(&self) -> StoreResult<redis::Connection> {
        self.client.get_connection().map_err(|e| {
            StoreError::ConnectionFailed(format!("Failed to get Redis connection: {}", e))
        })
    }

    /// Validate key is not empty
    fn validate_key(&self, key: &str) -> StoreResult<()> {
        if key.trim().is_empty() {
            return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
        }
        Ok(())
    }

    /// Validate field is not empty
    fn validate_field(&self, field: &str) -> StoreResult<()> {
        if field.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "Field cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl Store for RedisStore {
    async fn get<T: serde::de::DeserializeOwned + Send + Sync>(
        &self,
        key: &str,
    ) -> StoreResult<Option<T>> {
        self.validate_key(key)?;

        let mut con = self.get_connection().await?;

        let value: Option<String> = con
            .get(key)
            .map_err(|e| StoreError::RedisFailed(format!("Failed to get key '{}': {}", key, e)))?;

        match value {
            Some(v) => {
                let deserialized = self.deserialize(&v)?;
                Ok(Some(deserialized))
            }
            None => Ok(None),
        }
    }

    async fn set<T: serde::Serialize + Send + Sync>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> StoreResult<()> {
        self.validate_key(key)?;

        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        let mut con = self.get_connection().await?;

        let serialized_value = self.serialize(value)?;

        if let Some(ttl) = ttl {
            let _: () = con.set_ex(key, serialized_value, ttl).map_err(|e| {
                StoreError::RedisFailed(format!(
                    "Failed to set key '{}' with TTL {}: {}",
                    key, ttl, e
                ))
            })?;
        } else {
            let _: () = con.set(key, serialized_value).map_err(|e| {
                StoreError::RedisFailed(format!("Failed to set key '{}': {}", key, e))
            })?;
        }

        Ok(())
    }

    async fn delete(&self, key: &str) -> StoreResult<bool> {
        self.validate_key(key)?;

        let mut con = self.get_connection().await?;

        let result: i32 = con.del(key).map_err(|e| {
            StoreError::RedisFailed(format!("Failed to delete key '{}': {}", key, e))
        })?;

        Ok(result > 0)
    }

    async fn exists(&self, key: &str) -> StoreResult<bool> {
        self.validate_key(key)?;

        let mut con = self.get_connection().await?;

        let result: bool = con.exists(key).map_err(|e| {
            StoreError::RedisFailed(format!("Failed to check existence of key '{}': {}", key, e))
        })?;

        Ok(result)
    }

    async fn hset<T: serde::Serialize + Send + Sync>(
        &self,
        key: &str,
        field: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> StoreResult<bool> {
        self.validate_key(key)?;
        self.validate_field(field)?;

        if let Some(ttl_val) = ttl {
            if ttl_val == 0 {
                return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
            }
        }

        let mut con = self.get_connection().await?;

        let serialized_value = self.serialize(value)?;

        let hash_field_expiration_options = if let Some(ttl) = ttl {
            HashFieldExpirationOptions::default().set_expiration(SetExpiry::EX(ttl))
        } else {
            HashFieldExpirationOptions::default()
        };

        let fields_values = vec![(field, &serialized_value)];
        let result: i32 = con
            .hset_ex(key, &hash_field_expiration_options, &fields_values)
            .map_err(|e| {
                StoreError::RedisFailed(format!(
                    "Failed to set hash field '{}' in key '{}': {}",
                    field, key, e
                ))
            })?;

        Ok(result == 1)
    }

    async fn hget<T: serde::de::DeserializeOwned + Send + Sync>(
        &self,
        key: &str,
        field: &str,
    ) -> StoreResult<Option<T>> {
        self.validate_key(key)?;
        self.validate_field(field)?;

        let mut con = self.get_connection().await?;

        let value: Option<String> = con.hget(key, field).map_err(|e| {
            StoreError::RedisFailed(format!(
                "Failed to get hash field '{}' from key '{}': {}",
                field, key, e
            ))
        })?;

        match value {
            Some(v) => {
                let deserialized = self.deserialize(&v)?;
                Ok(Some(deserialized))
            }
            None => Ok(None),
        }
    }

    async fn hdel(&self, key: &str, field: &str) -> StoreResult<bool> {
        self.validate_key(key)?;
        self.validate_field(field)?;

        let mut con = self.get_connection().await?;

        let result: i32 = con.hdel(key, field).map_err(|e| {
            StoreError::RedisFailed(format!(
                "Failed to delete hash field '{}' from key '{}': {}",
                field, key, e
            ))
        })?;

        Ok(result > 0)
    }

    async fn hgetall<T: serde::de::DeserializeOwned + Send + Sync>(
        &self,
        key: &str,
    ) -> StoreResult<HashMap<String, T>> {
        self.validate_key(key)?;

        let mut con = self.get_connection().await?;

        let hash_data: HashMap<String, String> = con.hgetall(key).map_err(|e| {
            StoreError::RedisFailed(format!(
                "Failed to get all hash fields from key '{}': {}",
                key, e
            ))
        })?;

        let mut result = HashMap::new();
        let mut deserialization_errors = Vec::new();

        for (field, value) in hash_data {
            match self.deserialize(&value) {
                Ok(deserialized) => {
                    result.insert(field, deserialized);
                }
                Err(e) => {
                    deserialization_errors.push(format!("field '{}': {}", field, e));
                }
            }
        }

        if !deserialization_errors.is_empty() {
            return Err(StoreError::DeserializationFailed(format!(
                "Failed to deserialize some fields from key '{}': {}",
                key,
                deserialization_errors.join(", ")
            )));
        }

        Ok(result)
    }

    async fn hexists(&self, key: &str, field: &str) -> StoreResult<bool> {
        self.validate_key(key)?;
        self.validate_field(field)?;

        let mut con = self.get_connection().await?;

        let result: bool = con.hexists(key, field).map_err(|e| {
            StoreError::RedisFailed(format!(
                "Failed to check existence of hash field '{}' in key '{}': {}",
                field, key, e
            ))
        })?;

        Ok(result)
    }

    async fn hkeys(&self, key: &str) -> StoreResult<Vec<String>> {
        self.validate_key(key)?;

        let mut con = self.get_connection().await?;

        let result: Vec<String> = con.hkeys(key).map_err(|e| {
            StoreError::RedisFailed(format!("Failed to get hash keys from key '{}': {}", key, e))
        })?;

        Ok(result)
    }

    async fn hvals<T: serde::de::DeserializeOwned + Send + Sync>(
        &self,
        key: &str,
    ) -> StoreResult<Vec<T>> {
        self.validate_key(key)?;

        let mut con = self.get_connection().await?;

        let values: Vec<String> = con.hvals(key).map_err(|e| {
            StoreError::RedisFailed(format!(
                "Failed to get hash values from key '{}': {}",
                key, e
            ))
        })?;

        let mut result = Vec::new();
        let mut deserialization_errors = Vec::new();

        for (index, value) in values.iter().enumerate() {
            match self.deserialize(&value) {
                Ok(deserialized) => {
                    result.push(deserialized);
                }
                Err(e) => {
                    deserialization_errors.push(format!("index {}: {}", index, e));
                }
            }
        }

        if !deserialization_errors.is_empty() {
            return Err(StoreError::DeserializationFailed(format!(
                "Failed to deserialize some values from key '{}': {}",
                key,
                deserialization_errors.join(", ")
            )));
        }

        Ok(result)
    }

    async fn hlen(&self, key: &str) -> StoreResult<usize> {
        self.validate_key(key)?;

        let mut con = self.get_connection().await?;

        let len: i32 = con.hlen(key).map_err(|e| {
            StoreError::RedisFailed(format!(
                "Failed to get hash length from key '{}': {}",
                key, e
            ))
        })?;

        Ok(len as usize)
    }
}
