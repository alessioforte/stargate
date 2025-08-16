use crate::store::Store;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::sync::Arc;

pub struct MemoryStore {
    data: Arc<DashMap<String, (String, Option<DateTime<Utc>>)>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            data: Arc::new(DashMap::new()),
        }
    }
}

#[async_trait]
impl Store for MemoryStore {
    async fn get<T: serde::de::DeserializeOwned + Send + Sync>(&self, key: &str) -> Option<T> {
        if let Some(entry) = self.data.get(key) {
            let (value, exp) = entry.value();
            if let Some(date) = exp {
                if Utc::now() > *date {
                    return None; // Expired
                }
            }
            serde_json::from_str(value).ok()
        } else {
            None
        }
    }

    async fn set<T: serde::Serialize + Send + Sync>(&self, key: &str, value: &T, ttl: Option<u64>) {
        let data = serde_json::to_string(value).expect("Failed to serialize value");
        let exp = ttl.map(|t| Utc::now() + chrono::Duration::seconds(t as i64));
        self.data.insert(key.to_string(), (data, exp));
    }

    async fn delete(&self, key: &str) -> bool {
        self.data.remove(key).is_some()
    }
}

impl MemoryStore {
    pub fn run_cleaner(&self, interval: u64) {
        let data = self.data.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(interval));
            loop {
                ticker.tick().await;
                let now = Utc::now();
                let mut removed = 0;
                data.retain(|_, (_, exp)| {
                    let keep = exp.map_or(true, |exp| exp > now);
                    if !keep {
                        removed += 1;
                    }
                    keep
                });

                if removed > 0 {
                    log::info!("MemoryStore cleaner removed {} expired entries", removed);
                }
            }
        });
    }
}
