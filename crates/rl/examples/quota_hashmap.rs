//! Example demonstrating the simplified HashMap-based QuotaConfig API
//!
//! This example shows how to use the new QuotaConfig structure with
//! a direct HashMap<ResetPeriod, u64> for quota management.

use rl::{
    config::RateLimitConfig,
    quota::{QuotaConfig, ResetPeriod},
    quota_manager::QuotaManager,
};
use std::collections::HashMap;
use store::MemoryStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== QuotaConfig HashMap API Examples ===\n");

    // Example 1: Using builder methods
    builder_api_example().await?;

    // Example 2: Direct HashMap construction
    direct_hashmap_example().await?;

    // Example 3: Complex quota scenarios
    complex_scenarios_example().await?;

    Ok(())
}

/// Example 1: Using the fluent builder API
async fn builder_api_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("1. Builder API Example");
    println!("======================");

    let store = MemoryStore::new();
    let quota_manager = QuotaManager::new(store);

    // Build quota config using fluent API
    let quota_config = QuotaConfig::new()
        .with_daily(100)
        .with_weekly(500)
        .with_monthly(2000);

    let config = RateLimitConfig::default().with_quota(quota_config);

    println!(
        "Created config with {} quotas:",
        config.quota().unwrap().quotas.len()
    );
    for (period, limit) in &config.quota().unwrap().quotas {
        println!("  {:?}: {} requests", period, limit);
    }

    // Test quota usage
    let decision = quota_manager
        .check_and_consume_quota("user_builder", 25, &config)
        .await?;

    if decision.allowed {
        println!("\n✓ Request allowed (cost: 25)");

        if let Some(status) = quota_manager.get_quota_status("user_builder").await? {
            for (period, quota_status) in &status.periods {
                println!(
                    "  {:?}: {}/{} used ({:.1}%)",
                    period,
                    quota_status.used,
                    quota_status.limit,
                    quota_status.utilization() * 100.0
                );
            }
        }
    }

    println!();
    Ok(())
}

/// Example 2: Direct HashMap construction
async fn direct_hashmap_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("2. Direct HashMap Construction");
    println!("==============================");

    let store = MemoryStore::new();
    let quota_manager = QuotaManager::new(store);

    // Create HashMap directly
    let mut quotas = HashMap::new();
    quotas.insert(ResetPeriod::Daily, 50);
    quotas.insert(ResetPeriod::Weekly, 300);
    quotas.insert(ResetPeriod::Monthly, 1200);
    quotas.insert(ResetPeriod::Yearly, 12000);
    quotas.insert(ResetPeriod::Never, 50000); // Lifetime limit

    let quota_config = QuotaConfig { quotas };
    let config = RateLimitConfig::default().with_quota(quota_config);

    println!("Created config with direct HashMap:");
    for (period, limit) in &config.quota().unwrap().quotas {
        println!("  {:?}: {} requests", period, limit);
    }

    // Test with higher usage
    let operations = [
        ("Initial request", 10),
        ("Data sync", 20),
        ("Report generation", 15),
    ];

    for (operation, cost) in operations {
        let decision = quota_manager
            .check_and_consume_quota("user_direct", cost, &config)
            .await?;

        if decision.allowed {
            println!("\n✓ {} (cost: {}) - Allowed", operation, cost);
        } else {
            println!("\n✗ {} (cost: {}) - Denied", operation, cost);
            if let Some(violation) = &decision.violation_type {
                println!("  Violation: {:?}", violation);
            }
        }
    }

    println!();
    Ok(())
}

/// Example 3: Complex quota scenarios
async fn complex_scenarios_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("3. Complex Quota Scenarios");
    println!("==========================");

    let store = MemoryStore::new();
    let quota_manager = QuotaManager::new(store);

    // Scenario 1: Freemium user with tight limits
    let freemium_config = QuotaConfig::new()
        .with_daily(10) // Very limited daily usage
        .with_permanent(100); // Lifetime cap

    // Scenario 2: Enterprise user with generous limits
    let enterprise_config = QuotaConfig::new()
        .with_daily(10000)
        .with_weekly(50000)
        .with_monthly(200000)
        .with_yearly(2000000);

    // Scenario 3: Custom business cycle quotas
    let mut business_quotas = HashMap::new();
    business_quotas.insert(ResetPeriod::Daily, 1000); // Daily operations
    business_quotas.insert(ResetPeriod::Weekly, 6000); // Weekly reports
    business_quotas.insert(ResetPeriod::Monthly, 20000); // Monthly billing
    let business_config = QuotaConfig {
        quotas: business_quotas,
    };

    let scenarios = [
        ("Freemium User", freemium_config, "freemium_user"),
        ("Enterprise User", enterprise_config, "enterprise_user"),
        ("Business User", business_config, "business_user"),
    ];

    for (name, quota_config, user_key) in scenarios {
        println!("\n{} Configuration:", name);

        // Show quota limits
        for (period, limit) in &quota_config.quotas {
            println!("  {:?}: {} requests", period, limit);
        }

        let config = RateLimitConfig::default().with_quota(quota_config);

        // Simulate usage
        let usage_cost = match name {
            "Freemium User" => 5,     // Small request
            "Enterprise User" => 500, // Large batch
            "Business User" => 100,   // Medium operation
            _ => 10,
        };

        let decision = quota_manager
            .check_and_consume_quota(user_key, usage_cost, &config)
            .await?;

        if decision.allowed {
            println!("  ✓ Request (cost: {}) allowed", usage_cost);

            // Show remaining quotas
            if let Some(status) = quota_manager.get_quota_status(user_key).await? {
                for (period, quota_status) in &status.periods {
                    if quota_status.used > 0 {
                        println!(
                            "    {:?}: {}/{} remaining",
                            period, quota_status.remaining, quota_status.limit
                        );
                    }
                }
            }
        } else {
            println!("  ✗ Request (cost: {}) denied", usage_cost);
            if let Some(violation) = &decision.violation_type {
                println!("    Reason: {:?}", violation);
            }
        }
    }

    println!();
    Ok(())
}
