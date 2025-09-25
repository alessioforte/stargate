use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use store::{MemoryStore, Store, print_stats};
use tokio::time::{Duration, sleep};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct User {
    id: u32,
    name: String,
    email: String,
    age: u8,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Product {
    id: u32,
    name: String,
    price: f64,
    category: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let store = MemoryStore::new();

    // Start the cleaner with 30-second intervals
    store.run_cleaner(30);

    println!("=== Memory Store Stats Demo ===\n");

    // Initial stats
    println!("📊 Initial Stats:");
    print_stats(&store);

    // Add some simple key-value pairs
    println!("\n🔧 Adding simple key-value pairs...");

    let users = vec![
        User {
            id: 1,
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            age: 30,
        },
        User {
            id: 2,
            name: "Bob".to_string(),
            email: "bob@example.com".to_string(),
            age: 25,
        },
        User {
            id: 3,
            name: "Charlie".to_string(),
            email: "charlie@example.com".to_string(),
            age: 35,
        },
    ];

    for user in &users {
        store
            .set(&format!("user:{}", user.id), user, Some(300))
            .await; // 5 minutes TTL
    }

    // Add some products with shorter TTL
    let products = vec![
        Product {
            id: 1,
            name: "Laptop".to_string(),
            price: 999.99,
            category: "Electronics".to_string(),
        },
        Product {
            id: 2,
            name: "Book".to_string(),
            price: 19.99,
            category: "Education".to_string(),
        },
    ];

    for product in &products {
        store
            .set(&format!("product:{}", product.id), product, Some(60))
            .await; // 1 minute TTL
    }

    print_stats(&store);

    // Add hash data
    println!("\n🗂️  Adding hash data...");

    // User preferences hash
    store
        .hset("user:1:prefs", "theme", &"dark".to_string(), None)
        .await;
    store
        .hset("user:1:prefs", "language", &"en".to_string(), None)
        .await;
    store
        .hset("user:1:prefs", "notifications", &true, Some(120))
        .await; // 2 minutes TTL

    // Shopping cart hash
    store
        .hset("cart:user:1", "product:1", &2u32, Some(1800))
        .await; // 30 minutes TTL
    store
        .hset("cart:user:1", "product:2", &1u32, Some(1800))
        .await;

    // Session data hash with short TTL
    store
        .hset("session:abc123", "user_id", &1u32, Some(30))
        .await; // 30 seconds TTL
    store
        .hset(
            "session:abc123",
            "login_time",
            &chrono::Utc::now().to_rfc3339(),
            Some(30),
        )
        .await;

    print_stats(&store);

    // Perform various operations to generate stats
    println!("\n🔍 Performing various operations...");

    // Test cache hits and misses
    for i in 1..=3 {
        let user: Option<User> = store.get(&format!("user:{}", i)).await;
        if let Some(user) = user {
            println!("✅ Found user: {}", user.name);
        }
    }

    // Test non-existent keys (cache misses)
    for i in 10..=15 {
        let user: Option<User> = store.get(&format!("user:{}", i)).await;
        if user.is_none() {
            println!("❌ User {} not found", i);
        }
    }

    // Test hash operations
    let prefs: HashMap<String, String> = store.hgetall("user:1:prefs").await;
    println!("📋 User preferences: {:?}", prefs);

    let cart_keys = store.hkeys("cart:user:1").await;
    println!("🛒 Cart items: {:?}", cart_keys);

    let cart_size = store.hlen("cart:user:1").await;
    println!("📦 Cart size: {}", cart_size);

    // Test existence checks
    println!("🔎 Checking existence:");
    println!("  user:1 exists: {}", store.exists("user:1").await);
    println!("  user:999 exists: {}", store.exists("user:999").await);
    println!(
        "  user:1:prefs.theme exists: {}",
        store.hexists("user:1:prefs", "theme").await
    );
    println!(
        "  user:1:prefs.nonexistent exists: {}",
        store.hexists("user:1:prefs", "nonexistent").await
    );

    print_stats(&store);

    // Wait for some entries to expire
    println!("\n⏰ Waiting 35 seconds for some entries to expire...");
    sleep(Duration::from_secs(35)).await;

    // Try to access expired data
    println!("🔍 Checking expired data:");
    let expired_session: Option<String> = store.hget("session:abc123", "user_id").await;
    println!("  Expired session data: {:?}", expired_session);

    let expired_product: Option<Product> = store.get("product:1").await;
    println!("  Expired product: {:?}", expired_product);

    print_stats(&store);

    // Demonstrate memory usage with bulk data
    println!("\n💾 Adding bulk data to demonstrate memory usage...");

    for i in 100..200 {
        let bulk_user = User {
            id: i,
            name: format!("User{}", i),
            email: format!("user{}@example.com", i),
            age: (i % 80 + 18) as u8,
        };
        store
            .set(&format!("bulk:user:{}", i), &bulk_user, Some(600))
            .await;
    }

    // Add bulk hash data
    for i in 100..150 {
        store
            .hset(
                &format!("bulk:data:{}", i),
                "field1",
                &format!("value{}", i),
                Some(600),
            )
            .await;
        store
            .hset(&format!("bulk:data:{}", i), "field2", &(i * 2), Some(600))
            .await;
        store
            .hset(
                &format!("bulk:data:{}", i),
                "field3",
                &(i as f64 * 3.14),
                Some(600),
            )
            .await;
    }

    print_stats(&store);

    // Test cache hit ratio
    println!("\n📈 Testing cache hit ratio:");
    let initial_hit_ratio = store.get_cache_hit_ratio();
    println!("  Initial hit ratio: {:.2}%", initial_hit_ratio * 100.0);

    // Generate more hits
    for i in 100..120 {
        let _: Option<User> = store.get(&format!("bulk:user:{}", i)).await;
    }

    // Generate more misses
    for i in 1000..1020 {
        let _: Option<User> = store.get(&format!("nonexistent:{}", i)).await;
    }

    let final_hit_ratio = store.get_cache_hit_ratio();
    println!("  Final hit ratio: {:.2}%", final_hit_ratio * 100.0);

    print_stats(&store);

    // Clean up some data
    println!("\n🧹 Cleaning up data...");
    for i in 150..200 {
        store.delete(&format!("bulk:user:{}", i)).await;
    }

    // Delete some hash fields
    for i in 130..150 {
        store.hdel(&format!("bulk:data:{}", i), "field2").await;
    }

    print_stats(&store);

    // Test stats reset
    println!("\n🔄 Resetting stats...");
    store.reset_stats();

    println!("📊 Stats after reset:");
    print_stats(&store);

    // Final operations to show stats are working again
    println!("\n🔧 Final operations after reset...");
    for i in 1..=3 {
        let _: Option<User> = store.get(&format!("user:{}", i)).await;
    }

    print_stats(&store);

    println!("\n✅ Demo completed!");

    Ok(())
}

// fn print_stats(store: &MemoryStore) {
//     let stats = store.get_storage_stats();
//     let memory_mb = stats.estimated_memory_bytes as f64 / 1024.0 / 1024.0;
//     let hit_ratio = store.get_cache_hit_ratio();

//     println!("📊 Storage Stats:");
//     println!("  📁 Total Keys: {}", stats.total_keys);
//     println!("  🔑 Simple Keys: {}", stats.simple_keys);
//     println!("  🗂️  Hash Keys: {}", stats.hash_keys);
//     println!("  📋 Hash Fields: {}", stats.total_hash_fields);
//     println!(
//         "  💾 Memory Usage: {:.2} MB ({} bytes)",
//         memory_mb, stats.estimated_memory_bytes
//     );
//     println!("  🎯 Cache Hit Ratio: {:.2}%", hit_ratio * 100.0);

//     println!("🔧 Operation Stats:");
//     println!("  📖 Gets: {}", stats.operations.gets);
//     println!("  📝 Sets: {}", stats.operations.sets);
//     println!("  🗑️  Deletes: {}", stats.operations.deletes);
//     println!("  🔍 Exists Checks: {}", stats.operations.exists_checks);
//     println!("  📖 Hash Gets: {}", stats.operations.hash_gets);
//     println!("  📝 Hash Sets: {}", stats.operations.hash_sets);
//     println!("  🗑️  Hash Deletes: {}", stats.operations.hash_deletes);
//     println!("  🔍 Hash Exists: {}", stats.operations.hash_exists_checks);
//     println!("  📋 Hash GetAlls: {}", stats.operations.hash_getalls);
//     println!("  🔑 Hash Keys: {}", stats.operations.hash_keys_calls);
//     println!("  📄 Hash Vals: {}", stats.operations.hash_vals_calls);
//     println!("  📏 Hash Lens: {}", stats.operations.hash_len_calls);
//     println!("  ✅ Cache Hits: {}", stats.operations.cache_hits);
//     println!("  ❌ Cache Misses: {}", stats.operations.cache_misses);
//     println!(
//         "  🧹 Expired Cleaned: {}",
//         stats.operations.expired_entries_cleaned
//     );
//     println!();
// }
