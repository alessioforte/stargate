use store::{MemoryStore, Store};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
struct User {
    id: u32,
    name: String,
    email: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
struct Config {
    timeout: u64,
    retries: u32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a new memory store
    let store = MemoryStore::new();

    println!("=== Hash Operations Demo ===\n");

    // Create some test data
    let user1 = User {
        id: 1,
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
    };

    let user2 = User {
        id: 2,
        name: "Bob".to_string(),
        email: "bob@example.com".to_string(),
    };

    let config1 = Config {
        timeout: 30,
        retries: 3,
    };

    let config2 = Config {
        timeout: 60,
        retries: 5,
    };

    // Demo 1: Basic hash operations
    println!("1. Setting hash fields...");
    let is_new = store.hset("users", "alice", &user1, None).await;
    println!(
        "   Set users:alice -> {} (new field: {})",
        user1.name, is_new
    );

    let is_new = store.hset("users", "bob", &user2, None).await;
    println!("   Set users:bob -> {} (new field: {})", user2.name, is_new);

    let is_new = store.hset("config", "database", &config1, None).await;
    println!(
        "   Set config:database -> timeout: {} (new field: {})",
        config1.timeout, is_new
    );

    let is_new = store.hset("config", "cache", &config2, None).await;
    println!(
        "   Set config:cache -> timeout: {} (new field: {})",
        config2.timeout, is_new
    );

    // Demo 2: Getting individual hash fields
    println!("\n2. Getting individual hash fields...");
    if let Some(user) = store.hget::<User>("users", "alice").await {
        println!("   users:alice -> {:?}", user);
    }

    if let Some(config) = store.hget::<Config>("config", "database").await {
        println!("   config:database -> {:?}", config);
    }

    // Demo 3: Check field existence
    println!("\n3. Checking field existence...");
    println!(
        "   users:alice exists: {}",
        store.hexists("users", "alice").await
    );
    println!(
        "   users:charlie exists: {}",
        store.hexists("users", "charlie").await
    );

    // Demo 4: Get all keys and values
    println!("\n4. Getting all hash keys and values...");
    let user_keys = store.hkeys("users").await;
    println!("   User keys: {:?}", user_keys);

    let config_keys = store.hkeys("config").await;
    println!("   Config keys: {:?}", config_keys);

    let all_users: std::collections::HashMap<String, User> = store.hgetall("users").await;
    println!("   All users: {:#?}", all_users);

    // Demo 5: Get hash length
    println!("\n5. Getting hash lengths...");
    println!("   Users hash length: {}", store.hlen("users").await);
    println!("   Config hash length: {}", store.hlen("config").await);
    println!(
        "   Non-existent hash length: {}",
        store.hlen("nonexistent").await
    );

    // Demo 6: Update existing field
    println!("\n6. Updating existing field...");
    let mut updated_user = user1.clone();
    updated_user.email = "alice.updated@example.com".to_string();
    let is_new = store.hset("users", "alice", &updated_user, None).await;
    println!("   Updated users:alice email (new field: {})", is_new);

    if let Some(user) = store.hget::<User>("users", "alice").await {
        println!("   Updated user: {:?}", user);
    }

    // Demo 7: Delete hash field
    println!("\n7. Deleting hash field...");
    let deleted = store.hdel("users", "bob").await;
    println!("   Deleted users:bob: {}", deleted);
    println!(
        "   Users hash length after deletion: {}",
        store.hlen("users").await
    );

    // Demo 8: Try to delete non-existent field
    let deleted = store.hdel("users", "nonexistent").await;
    println!("   Tried to delete non-existent field: {}", deleted);

    // Demo 9: Get values only
    println!("\n8. Getting all values from hash...");
    let user_values: Vec<User> = store.hvals("users").await;
    println!("   User values: {:#?}", user_values);

    // Demo 9: TTL (Time-To-Live) functionality for hash fields
    println!("\n9. Demonstrating TTL functionality for hash fields...");

    // Set a field that expires in 2 seconds
    let temp_user = User {
        id: 999,
        name: "Temporary".to_string(),
        email: "temp@example.com".to_string(),
    };

    let is_new = store.hset("users", "temp", &temp_user, Some(2)).await;
    println!(
        "   Set users:temp with 2 second TTL (new field: {})",
        is_new
    );

    // Check that it exists immediately
    println!(
        "   temp field exists: {}",
        store.hexists("users", "temp").await
    );
    println!("   Users hash length: {}", store.hlen("users").await);

    // Wait for 1 second
    println!("   Waiting 1 second...");
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    println!(
        "   temp field still exists: {}",
        store.hexists("users", "temp").await
    );

    // Wait for another 2 seconds (total 3 seconds, should be expired)
    println!("   Waiting 2 more seconds...");
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    println!(
        "   temp field exists after expiration: {}",
        store.hexists("users", "temp").await
    );
    println!(
        "   Users hash length after expiration: {}",
        store.hlen("users").await
    );

    // Set another field with longer TTL and one without TTL
    let long_lived_user = User {
        id: 888,
        name: "LongLived".to_string(),
        email: "long@example.com".to_string(),
    };

    store
        .hset("users", "long_lived", &long_lived_user, Some(10))
        .await;
    store.hset("users", "permanent", &user1, None).await;

    println!("   Set long_lived (10s TTL) and permanent (no TTL)");
    println!("   Users hash length: {}", store.hlen("users").await);

    let all_keys = store.hkeys("users").await;
    println!("   Current user keys: {:?}", all_keys);

    // Demo 10: Demonstrate separation from regular key-value operations
    println!("\n10. Demonstrating separation from regular key-value operations...");

    // Set a regular key-value pair with a different key
    store
        .set("simple_key", &"This is a regular string value", None)
        .await;

    // Try to get it as a regular value (should work)
    if let Some(regular_value) = store.get::<String>("simple_key").await {
        println!("   Regular 'simple_key' value: {}", regular_value);
    }

    // Hash operations should not work on this simple key
    println!(
        "   Hash length on simple key: {}",
        store.hlen("simple_key").await
    );
    println!(
        "   Hash field exists on simple key: {}",
        store.hexists("simple_key", "field").await
    );

    // Conversely, regular get should not work on hash keys
    let regular_value: Option<String> = store.get("users").await;
    println!(
        "   Regular get on hash key 'users' returns: {:?}",
        regular_value
    );

    // Show that our hash still works
    println!(
        "   Hash 'users' still has length: {}",
        store.hlen("users").await
    );

    println!("\n=== Demo Complete ===");
    Ok(())
}
