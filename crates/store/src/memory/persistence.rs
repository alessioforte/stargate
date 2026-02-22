use super::memory::{MemoryStore, StoreValue};
use super::stats::OperationStats;
use crate::error::{StoreError, StoreResult};
use crate::store::Store;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use portable_atomic::AtomicI64;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::fs;

// Serializable data structures for JSON persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
enum SerializableStoreValue {
    Simple(serde_json::Value, Option<DateTime<Utc>>),
    Hash(
        HashMap<String, (serde_json::Value, Option<DateTime<Utc>>)>,
        Option<DateTime<Utc>>,
    ),
    AtomicI64(i64, Option<DateTime<Utc>>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerializableStoreData {
    data: HashMap<String, SerializableStoreValue>,
    stats: OperationStats,
}

impl MemoryStore {
    /// Helper function to deserialize stored MessagePack bytes into a serde_json::Value
    /// for human-readable JSON persistence.
    fn parse_stored_value(value: &[u8]) -> serde_json::Value {
        rmp_serde::from_slice(value).unwrap_or(serde_json::Value::Null)
    }

    /// Converts the current store data to a serializable format
    fn to_serializable(&self) -> SerializableStoreData {
        let mut serializable_data = HashMap::new();

        for entry in self.data.iter() {
            let key = entry.key().clone();
            let value = match entry.value() {
                StoreValue::Simple(val, exp) => {
                    let json_val = Self::parse_stored_value(val);
                    SerializableStoreValue::Simple(json_val, *exp)
                }
                StoreValue::Hash(hash_map, exp) => {
                    let mut hash_data = HashMap::new();
                    for hash_entry in hash_map.iter() {
                        let (val, field_exp) = hash_entry.value();
                        let json_val = Self::parse_stored_value(val);
                        hash_data.insert(hash_entry.key().clone(), (json_val, *field_exp));
                    }
                    SerializableStoreValue::Hash(hash_data, *exp)
                }
                StoreValue::AtomicI64(val, exp) => {
                    let num: i64 = val.load(std::sync::atomic::Ordering::SeqCst);
                    SerializableStoreValue::AtomicI64(num, *exp)
                }
            };
            serializable_data.insert(key, value);
        }

        SerializableStoreData {
            data: serializable_data,
            stats: self.stats.to_operation_stats(),
        }
    }

    /// Creates a new MemoryStore from serializable data
    fn from_serializable(data: SerializableStoreData) -> Self {
        let store = MemoryStore::new();

        for (key, value) in data.data {
            let store_value = match value {
                SerializableStoreValue::Simple(val, exp) => {
                    let data = rmp_serde::to_vec(&val).unwrap_or_default().into();
                    StoreValue::Simple(data, exp)
                }
                SerializableStoreValue::Hash(hash_data, exp) => {
                    let dash_map = DashMap::new();
                    for (field, (val, field_exp)) in hash_data {
                        let data = rmp_serde::to_vec(&val).unwrap_or_default().into();
                        dash_map.insert(field, (data, field_exp));
                    }
                    StoreValue::Hash(dash_map, exp)
                }
                SerializableStoreValue::AtomicI64(val, exp) => {
                    StoreValue::AtomicI64(Arc::new(AtomicI64::new(val)), exp)
                }
            };
            store.data.insert(key, store_value);
        }

        // Note: We don't restore atomic stats as they should start fresh
        // The historical stats are available in the serialized data if needed

        store
    }

    /// Ensures the parent directory for a file path exists, creating it if necessary
    async fn ensure_file_path_exists<P: AsRef<Path>>(path: P) -> StoreResult<()> {
        let path = path.as_ref();

        // Check if the file already exists
        if path.exists() {
            return Ok(());
        }

        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).await.map_err(|e| {
                    StoreError::InvalidInput(format!("Failed to create parent directories: {}", e))
                })?;
            }
        }

        Ok(())
    }

    /// Performs an atomic write by writing to a temporary file and then renaming it
    async fn atomic_write<P: AsRef<Path>>(path: P, content: String) -> StoreResult<()> {
        let path = path.as_ref();
        let temp_path = path.with_extension(format!(
            "{}.tmp",
            path.extension().and_then(|s| s.to_str()).unwrap_or("json")
        ));

        // Write to temporary file first
        fs::write(&temp_path, content).await.map_err(|e| {
            StoreError::InvalidInput(format!("Failed to write to temporary file: {}", e))
        })?;

        // Atomically rename temp file to target file
        fs::rename(&temp_path, path).await.map_err(|e| {
            // Try to clean up temp file if rename fails
            let _ = std::fs::remove_file(&temp_path);
            StoreError::InvalidInput(format!("Failed to rename temporary file: {}", e))
        })?;

        Ok(())
    }

    /// Checks if a file exists and returns metadata about it
    pub async fn file_info<P: AsRef<Path>>(path: P) -> StoreResult<Option<(u64, DateTime<Utc>)>> {
        let path = path.as_ref();

        match fs::metadata(path).await {
            Ok(metadata) => {
                let size = metadata.len();
                let modified = metadata
                    .modified()
                    .map_err(|e| {
                        StoreError::InvalidInput(format!("Failed to get modification time: {}", e))
                    })?
                    .into();
                Ok(Some((size, modified)))
            }
            Err(_) => Ok(None), // File doesn't exist
        }
    }

    /// Creates a backup of an existing file before overwriting it
    pub async fn backup_file<P: AsRef<Path>>(path: P) -> StoreResult<Option<String>> {
        let path = path.as_ref();

        if !path.exists() {
            return Ok(None);
        }

        let backup_path = path.with_extension(format!(
            "{}.backup.{}",
            path.extension().and_then(|s| s.to_str()).unwrap_or("json"),
            chrono::Utc::now().format("%Y%m%d_%H%M%S")
        ));

        fs::copy(path, &backup_path)
            .await
            .map_err(|e| StoreError::InvalidInput(format!("Failed to create backup: {}", e)))?;

        Ok(Some(backup_path.to_string_lossy().to_string()))
    }

    /// Saves the store data to a JSON file
    pub async fn save_to_json<P: AsRef<Path>>(&self, path: P) -> StoreResult<()> {
        // Ensure the file path exists
        Self::ensure_file_path_exists(&path).await?;

        let serializable_data = self.to_serializable();
        let json_string = serde_json::to_string_pretty(&serializable_data).map_err(|e| {
            StoreError::SerializationFailed(format!("Failed to serialize store data: {}", e))
        })?;

        Self::atomic_write(path, json_string).await
    }

    /// Saves the store with automatic backup of existing file
    pub async fn save_to_json_with_backup<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> StoreResult<Option<String>> {
        let backup_path = Self::backup_file(&path).await?;
        self.save_to_json(path).await?;
        Ok(backup_path)
    }

    /// Loads store data from a JSON file
    pub async fn load_from_json<P: AsRef<Path>>(path: P) -> StoreResult<Self> {
        let json_string = fs::read_to_string(path)
            .await
            .map_err(|e| StoreError::InvalidInput(format!("Failed to read from file: {}", e)))?;

        let serializable_data: SerializableStoreData =
            serde_json::from_str(&json_string).map_err(|e| {
                StoreError::DeserializationFailed(format!(
                    "Failed to deserialize store data: {}",
                    e
                ))
            })?;

        Ok(Self::from_serializable(serializable_data))
    }

    /// Loads store data from the simple JSON export format.
    ///
    /// The simple format does not contain type metadata for integers,
    /// so numeric values are restored as simple values (not AtomicI64).
    pub async fn load_from_simple_json<P: AsRef<Path>>(path: P) -> StoreResult<Self> {
        let json_string = fs::read_to_string(path)
            .await
            .map_err(|e| StoreError::InvalidInput(format!("Failed to read from file: {}", e)))?;

        let export_data: HashMap<String, serde_json::Value> = serde_json::from_str(&json_string)
            .map_err(|e| {
                StoreError::DeserializationFailed(format!(
                    "Failed to deserialize simple store data: {}",
                    e
                ))
            })?;

        let store = MemoryStore::new();

        for (key, value) in export_data {
            match value {
                serde_json::Value::Object(object_fields) => {
                    let hash_map = DashMap::new();
                    for (field, field_value) in object_fields {
                        let data = rmp_serde::to_vec(&field_value).map_err(|e| {
                            StoreError::SerializationFailed(format!(
                                "Failed to serialize hash field '{}' for key '{}': {}",
                                field, key, e
                            ))
                        })?;
                        hash_map.insert(field, (data.into(), None));
                    }
                    store.data.insert(key, StoreValue::Hash(hash_map, None));
                }
                other => {
                    let data = rmp_serde::to_vec(&other).map_err(|e| {
                        StoreError::SerializationFailed(format!(
                            "Failed to serialize value for key '{}': {}",
                            key, e
                        ))
                    })?;
                    store
                        .data
                        .insert(key, StoreValue::Simple(data.into(), None));
                }
            }
        }

        Ok(store)
    }

    /// Load store data preferring full JSON format and falling back to the
    /// simple JSON backup format for backward compatibility.
    pub async fn load_from_json_with_fallback<P: AsRef<Path>>(path: P) -> StoreResult<Self> {
        let path = path.as_ref();
        match Self::load_from_json(path).await {
            Ok(store) => Ok(store),
            Err(StoreError::DeserializationFailed(_)) => Self::load_from_simple_json(path).await,
            Err(err) => Err(err),
        }
    }

    /// Saves only the keys and values to a JSON file (without expiration times and stats)
    /// This creates a simpler, human-readable JSON format
    pub async fn export_to_simple_json<P: AsRef<Path>>(&self, path: P) -> StoreResult<()> {
        // Ensure the file path exists
        Self::ensure_file_path_exists(&path).await?;

        let mut export_data = HashMap::new();

        for entry in self.data.iter() {
            let key = entry.key().clone();
            match entry.value() {
                StoreValue::Simple(val, exp) => {
                    // Only include non-expired simple values
                    if !self.is_expired(exp) {
                        let json_val = Self::parse_stored_value(val);
                        export_data.insert(key, json_val);
                    }
                }
                StoreValue::Hash(hash_map, exp) => {
                    // Only include non-expired hash values
                    if !self.is_expired(exp) {
                        let mut hash_export = HashMap::new();
                        for hash_entry in hash_map.iter() {
                            let (field_val, field_exp) = hash_entry.value();
                            if !self.is_expired(field_exp) {
                                let json_val = Self::parse_stored_value(field_val);
                                hash_export.insert(hash_entry.key().clone(), json_val);
                            }
                        }
                        if !hash_export.is_empty() {
                            export_data.insert(
                                key,
                                serde_json::Value::Object(hash_export.into_iter().collect()),
                            );
                        }
                    }
                }
                StoreValue::AtomicI64(val, exp) => {
                    // Only include non-expired atomic values
                    if !self.is_expired(exp) {
                        let num: i64 = val.load(std::sync::atomic::Ordering::SeqCst);
                        export_data.insert(key, serde_json::Value::Number(num.into()));
                    }
                }
            }
        }

        let json_string = serde_json::to_string_pretty(&export_data).map_err(|e| {
            StoreError::SerializationFailed(format!("Failed to serialize export data: {}", e))
        })?;

        Self::atomic_write(path, json_string).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_file_operations() -> StoreResult<()> {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join(format!(
            "test_store_{}.json",
            chrono::Utc::now().timestamp()
        ));

        // Create a store with some data
        let store = MemoryStore::new();
        store.set("key1", &"value1", None).await?;
        store.hset("hash1", "field1", &"hvalue1", None).await?;

        // Test file doesn't exist initially
        assert!(MemoryStore::file_info(&file_path).await?.is_none());

        // Save to JSON (will create file and directories)
        store.save_to_json(&file_path).await?;

        // Verify file exists and has content
        let (size, _modified) = MemoryStore::file_info(&file_path).await?.unwrap();
        assert!(size > 0);

        // Load from JSON
        let loaded_store = MemoryStore::load_from_json(&file_path).await?;
        let value: Option<String> = loaded_store.get("key1").await?;
        assert_eq!(value, Some("value1".to_string()));

        // Test simple export
        let simple_path = temp_dir.join("simple.json");
        store.export_to_simple_json(&simple_path).await?;

        // Verify simple export file exists
        assert!(MemoryStore::file_info(&simple_path).await?.is_some());

        Ok(())
    }

    #[tokio::test]
    async fn test_nested_directory_creation() -> StoreResult<()> {
        let temp_dir = std::env::temp_dir();
        let nested_path = temp_dir
            .join(format!("test_{}", chrono::Utc::now().timestamp()))
            .join("level1")
            .join("level2")
            .join("store.json");

        let store = MemoryStore::new();
        store.set("test", &"data", None).await?;

        // This should create all parent directories
        store.save_to_json(&nested_path).await?;

        // Verify file exists
        assert!(nested_path.exists());

        Ok(())
    }

    #[tokio::test]
    async fn test_backup_functionality() -> StoreResult<()> {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join(format!("store_{}.json", chrono::Utc::now().timestamp()));

        let store = MemoryStore::new();
        store.set("original", &"data", None).await?;

        // Save initial file
        store.save_to_json(&file_path).await?;

        // Modify store
        store.set("new", &"data", None).await?;

        // Save with backup
        let backup_path = store.save_to_json_with_backup(&file_path).await?;

        // Verify backup was created
        assert!(backup_path.is_some());
        if let Some(backup_path_str) = backup_path {
            let backup_file = std::path::Path::new(&backup_path_str);
            assert!(backup_file.exists());
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_json_value_parsing() -> StoreResult<()> {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join(format!(
            "test_json_values_{}.json",
            chrono::Utc::now().timestamp()
        ));

        let store = MemoryStore::new();

        // Test different JSON value types
        store.set("string_val", &"Hello World", None).await?;
        store.set("number_val", &"42", None).await?;
        store.set("float_val", &"3.14", None).await?;
        store.set("bool_true", &"true", None).await?;
        store.set("bool_false", &"false", None).await?;
        store.set("null_val", &"null", None).await?;
        store
            .set("json_obj", &r#"{"name":"Alice","age":30}"#, None)
            .await?;
        store.set("json_array", &r#"[1,2,3,"test"]"#, None).await?;

        // Export to simple JSON
        store.export_to_simple_json(&file_path).await?;

        // Read the exported JSON and verify structure
        let json_content = tokio::fs::read_to_string(&file_path).await?;
        let parsed: serde_json::Value = serde_json::from_str(&json_content)?;

        // Verify that values are parsed as proper JSON types
        assert_eq!(
            parsed["string_val"],
            serde_json::Value::String("Hello World".to_string())
        );
        assert_eq!(
            parsed["number_val"],
            serde_json::Value::String("42".to_string())
        );
        assert_eq!(
            parsed["bool_true"],
            serde_json::Value::String("true".to_string())
        );
        assert_eq!(
            parsed["null_val"],
            serde_json::Value::String("null".to_string())
        );

        // JSON objects and arrays should be parsed as strings since they're stored as JSON strings
        assert!(parsed["json_obj"].is_string());
        assert!(parsed["json_array"].is_string());

        Ok(())
    }

    #[tokio::test]
    async fn test_load_from_json_with_fallback_from_simple_export() -> StoreResult<()> {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join(format!(
            "test_simple_restore_{}.json",
            chrono::Utc::now().timestamp()
        ));

        let store = MemoryStore::new();
        store.set("key1", &"value1", None).await?;
        store.hset("hash1", "field1", &123_i32, None).await?;

        // Legacy backup format in the app currently uses the simple JSON export.
        store.export_to_simple_json(&file_path).await?;

        let restored_store = MemoryStore::load_from_json_with_fallback(&file_path).await?;
        let value: Option<String> = restored_store.get("key1").await?;
        assert_eq!(value, Some("value1".to_string()));

        let hash_value: Option<i32> = restored_store.hget("hash1", "field1").await?;
        assert_eq!(hash_value, Some(123));

        Ok(())
    }
}

// Example usage documentation
#[allow(dead_code)]
mod examples {
    use super::*;

    /// Example of basic file operations
    pub async fn basic_persistence_example() -> StoreResult<()> {
        let store = MemoryStore::new();

        // Add some data
        store.set("user:1", &"John Doe", None).await?;
        store.set("user:2", &"Jane Smith", Some(3600)).await?; // With TTL

        // Add hash data
        store.hset("session:abc", "user_id", &"1", None).await?;
        store
            .hset("session:abc", "ip", &"192.168.1.1", None)
            .await?;

        // Save with automatic directory creation
        let path = "data/backups/store.json";
        store.save_to_json(path).await?;

        // Check file info
        if let Some((size, modified)) = MemoryStore::file_info(path).await? {
            println!("File size: {} bytes, modified: {}", size, modified);
        }

        // Create a backup before overwriting
        if let Some(backup_path) = store.save_to_json_with_backup(path).await? {
            println!("Backup created: {}", backup_path);
        }

        // Export simple format for human reading
        store
            .export_to_simple_json("data/exports/simple.json")
            .await?;

        // Load from file
        let restored_store = MemoryStore::load_from_json(path).await?;
        let user: Option<String> = restored_store.get("user:1").await?;
        println!("Restored user: {:?}", user);

        Ok(())
    }
}
