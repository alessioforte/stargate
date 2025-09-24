use crate::store::Store;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;

use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
enum StoreValue {
    Simple(String, Option<DateTime<Utc>>),
    Hash(
        DashMap<String, (String, Option<DateTime<Utc>>)>,
        Option<DateTime<Utc>>,
    ),
}

pub struct MemoryStore {
    data: Arc<DashMap<String, StoreValue>>,
}

impl Clone for MemoryStore {
    fn clone(&self) -> Self {
        MemoryStore {
            data: Arc::clone(&self.data),
        }
    }
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            data: Arc::new(DashMap::new()),
        }
    }

    fn is_expired(&self, exp: &Option<DateTime<Utc>>) -> bool {
        exp.map_or(false, |date| Utc::now() > date)
    }
}

#[async_trait]
impl Store for MemoryStore {
    async fn get<T: serde::de::DeserializeOwned + Send + Sync>(&self, key: &str) -> Option<T> {
        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Simple(value, exp) => {
                    if self.is_expired(exp) {
                        return None;
                    }
                    serde_json::from_str(value).ok()
                }
                StoreValue::Hash(_, _) => None, // Cannot get hash as simple value
            }
        } else {
            None
        }
    }

    async fn set<T: serde::Serialize + Send + Sync>(&self, key: &str, value: &T, ttl: Option<u64>) {
        let data = serde_json::to_string(value).expect("Failed to serialize value");
        let exp = ttl.map(|t| Utc::now() + chrono::Duration::seconds(t as i64));
        self.data
            .insert(key.to_string(), StoreValue::Simple(data, exp));
    }

    async fn delete(&self, key: &str) -> bool {
        self.data.remove(key).is_some()
    }

    async fn exists(&self, key: &str) -> bool {
        if let Some(entry) = self.data.get(key) {
            let exp = match entry.value() {
                StoreValue::Simple(_, exp) => exp,
                StoreValue::Hash(_, exp) => exp,
            };
            !self.is_expired(exp)
        } else {
            false
        }
    }

    async fn hset<T: serde::Serialize + Send + Sync>(
        &self,
        key: &str,
        field: &str,
        value: &T,
        ttl: Option<u64>,
    ) -> bool {
        let serialized_value = serde_json::to_string(value).expect("Failed to serialize value");
        let field_exp = ttl.map(|t| Utc::now() + chrono::Duration::seconds(t as i64));

        if let Some(mut entry) = self.data.get_mut(key) {
            match entry.value_mut() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        return false;
                    }
                    let is_new = !hash_map.contains_key(field);
                    hash_map.insert(field.to_string(), (serialized_value, field_exp));
                    is_new
                }
                StoreValue::Simple(_, _) => false, // Cannot set hash field on simple value
            }
        } else {
            let hash_map = DashMap::new();
            hash_map.insert(field.to_string(), (serialized_value, field_exp));
            self.data
                .insert(key.to_string(), StoreValue::Hash(hash_map, None));
            true
        }
    }

    async fn hget<T: serde::de::DeserializeOwned + Send + Sync>(
        &self,
        key: &str,
        field: &str,
    ) -> Option<T> {
        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        return None;
                    }
                    if let Some(value_entry) = hash_map.get(field) {
                        let (value, field_exp) = value_entry.value();
                        if self.is_expired(field_exp) {
                            return None;
                        }
                        serde_json::from_str(value).ok()
                    } else {
                        None
                    }
                }
                StoreValue::Simple(_, _) => None, // Cannot get hash field from simple value
            }
        } else {
            None
        }
    }

    async fn hdel(&self, key: &str, field: &str) -> bool {
        if let Some(mut entry) = self.data.get_mut(key) {
            match entry.value_mut() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        return false;
                    }
                    hash_map.remove(field).is_some()
                }
                StoreValue::Simple(_, _) => false, // Cannot delete hash field from simple value
            }
        } else {
            false
        }
    }

    async fn hgetall<T: serde::de::DeserializeOwned + Send + Sync>(
        &self,
        key: &str,
    ) -> HashMap<String, T> {
        let mut result = HashMap::new();

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if !self.is_expired(exp) {
                        for item in hash_map.iter() {
                            let (field, (value, field_exp)) = item.pair();
                            if !self.is_expired(field_exp) {
                                if let Ok(deserialized) = serde_json::from_str::<T>(value) {
                                    result.insert(field.clone(), deserialized);
                                }
                            }
                        }
                    }
                }
                StoreValue::Simple(_, _) => {} // Cannot get all hash fields from simple value
            }
        }

        result
    }

    async fn hexists(&self, key: &str, field: &str) -> bool {
        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        return false;
                    }
                    if let Some(entry) = hash_map.get(field) {
                        let (_, field_exp) = entry.value();
                        !self.is_expired(field_exp)
                    } else {
                        false
                    }
                }
                StoreValue::Simple(_, _) => false, // Cannot check hash field existence in simple value
            }
        } else {
            false
        }
    }

    async fn hkeys(&self, key: &str) -> Vec<String> {
        let mut keys = Vec::new();

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if !self.is_expired(exp) {
                        for item in hash_map.iter() {
                            let (field, (_, field_exp)) = item.pair();
                            if !self.is_expired(field_exp) {
                                keys.push(field.clone());
                            }
                        }
                    }
                }
                StoreValue::Simple(_, _) => {} // Cannot get hash keys from simple value
            }
        }

        keys
    }

    async fn hvals<T: serde::de::DeserializeOwned + Send + Sync>(&self, key: &str) -> Vec<T> {
        let mut values = Vec::new();

        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if !self.is_expired(exp) {
                        for item in hash_map.iter() {
                            let (_, (value, field_exp)) = item.pair();
                            if !self.is_expired(field_exp) {
                                if let Ok(deserialized) = serde_json::from_str::<T>(value) {
                                    values.push(deserialized);
                                }
                            }
                        }
                    }
                }
                StoreValue::Simple(_, _) => {} // Cannot get hash values from simple value
            }
        }

        values
    }

    async fn hlen(&self, key: &str) -> usize {
        if let Some(entry) = self.data.get(key) {
            match entry.value() {
                StoreValue::Hash(hash_map, exp) => {
                    if self.is_expired(exp) {
                        0
                    } else {
                        hash_map
                            .iter()
                            .filter(|item| {
                                let (_, (_, field_exp)) = item.pair();
                                !self.is_expired(field_exp)
                            })
                            .count()
                    }
                }
                StoreValue::Simple(_, _) => 0, // Cannot get hash length from simple value
            }
        } else {
            0
        }
    }
}

impl MemoryStore {
    pub fn run_cleaner(&self, interval: u64) {
        let data = self.data.clone();
        tokio::spawn(async move {
            log::info!(
                "MemoryStore cleaner started with interval: {} seconds",
                interval
            );
            let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(interval));
            loop {
                ticker.tick().await;
                let now = Utc::now();
                let mut removed = 0;
                data.retain(|_, value| {
                    match value {
                        StoreValue::Simple(_, exp) => {
                            let keep = exp.map_or(true, |exp| exp > now);
                            if !keep {
                                removed += 1;
                            }
                            keep
                        }
                        StoreValue::Hash(hash_map, exp) => {
                            // Clean expired fields within the hash
                            hash_map.retain(|_, (_, field_exp)| {
                                let keep = field_exp.map_or(true, |exp| exp > now);
                                if !keep {
                                    removed += 1;
                                }
                                keep
                            });

                            // Keep the hash if it's not expired and has fields
                            let keep_hash =
                                exp.map_or(true, |exp| exp > now) && !hash_map.is_empty();
                            if !keep_hash && hash_map.is_empty() {
                                removed += 1;
                            }
                            keep_hash
                        }
                    }
                });

                if removed > 0 {
                    log::info!("MemoryStore cleaner removed {} expired entries", removed);
                }
            }
        });
    }
}
