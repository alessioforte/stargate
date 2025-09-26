//! Example demonstrating the separated rate limiting and quota management architecture
//!
//! This example shows how to use:
//! 1. RateLimiter for token bucket rate limiting
//! 2. QuotaManager for daily/monthly quota tracking
//! 3. UnifiedLimiter for combined rate limiting and quota management

use std::net::IpAddr;
use std::time::Duration;

use rl::{
    DenialReason, config::RateLimitConfig, limiter::RateLimiter, quota::QuotaConfig,
    quota_manager::QuotaManager, unified_limiter::UnifiedLimiter,
};
use store::MemoryStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Rate Limiting and Quota Management Examples ===\n");

    // Example 1: Using RateLimiter alone for token bucket rate limiting
    rate_limiter_example().await?;

    // Example 2: Using QuotaManager alone for quota tracking
    quota_manager_example().await?;

    // Example 3: Using UnifiedLimiter for combined rate limiting and quotas
    unified_limiter_example().await?;

    // Example 4: API Gateway usage pattern
    api_gateway_example().await?;

    Ok(())
}

/// Example 1: Pure rate limiting with token bucket
async fn rate_limiter_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("1. Rate Limiter Example (Token Bucket)");
    println!("=====================================");

    let store = MemoryStore::new();
    let limiter = RateLimiter::with_store(store);

    // Configure a custom rate limit: 10 tokens/sec, burst of 20
    let config = RateLimitConfig::new(10, 20, 60);
    limiter.set_config("api_key_123", config, None).await?;

    // Simulate API requests with different costs
    let requests = [
        ("Simple GET", 1),
        ("Complex query", 5),
        ("Bulk operation", 10),
        ("AI request", 15),
    ];

    for (operation, cost) in requests {
        let decision = limiter
            .check_rate_limit_tokens("api_key_123", cost as f64, None)
            .await?;

        if decision.allowed {
            println!(
                "✓ {} (cost: {}) - Allowed, {} tokens remaining",
                operation, cost, decision.remaining_tokens
            );
        } else {
            println!(
                "✗ {} (cost: {}) - Rate limited, retry after {}ms",
                operation,
                cost,
                decision.retry_after_ms.unwrap_or(0)
            );
        }
    }

    // IP-based rate limiting
    let ip: IpAddr = "192.168.1.100".parse()?;
    let ip_decision = limiter.check_rate_limit_ip(ip).await?;
    println!(
        "IP {} rate limit status: {}",
        ip,
        if ip_decision.allowed {
            "Allowed"
        } else {
            "Denied"
        }
    );

    println!();
    Ok(())
}

/// Example 2: Pure quota management
async fn quota_manager_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("2. Quota Manager Example");
    println!("========================");

    let store = MemoryStore::new();
    let quota_manager = QuotaManager::new(store);

    // Configure quotas: 1000 requests/day, 30000 requests/month
    let config = RateLimitConfig::default().with_quota(QuotaConfig::both(1000, 30000));

    // Simulate various operations with different costs
    let operations = [
        ("User login", 1),
        ("File upload", 10),
        ("AI generation", 50),
        ("Bulk data export", 100),
    ];

    for (operation, cost) in operations {
        let decision = quota_manager
            .check_and_consume_quota("user_456", cost, &config)
            .await?;

        if decision.allowed {
            println!("✓ {} (cost: {}) - Within quota", operation, cost);

            // Show quota status
            if let Some(status) = quota_manager.get_quota_status("user_456").await? {
                if let Some(daily) = &status.daily {
                    println!(
                        "  Daily: {}/{} used ({:.1}% utilization)",
                        daily.used,
                        daily.limit,
                        (daily.used as f64 / daily.limit as f64) * 100.0
                    );
                }
            }
        } else {
            let violation_type = if decision.is_daily_violation() {
                "daily"
            } else {
                "monthly"
            };
            println!(
                "✗ {} (cost: {}) - {} quota exceeded",
                operation, cost, violation_type
            );
        }
    }

    println!();
    Ok(())
}

/// Example 3: Combined rate limiting and quota management
async fn unified_limiter_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("3. Unified Limiter Example (Rate Limiting + Quotas)");
    println!("===================================================");

    let store = MemoryStore::new();
    let limiter = UnifiedLimiter::with_store(store);

    // Configure both rate limiting and quotas
    let config = RateLimitConfig::new(5, 10, 60) // 5 tokens/sec, burst of 10
        .with_quota(QuotaConfig::daily_only(200)); // 200 cost units/day

    limiter
        .set_config("premium_user", config.clone(), None)
        .await?;

    // Simulate high-frequency, high-cost requests
    let requests = [
        ("Regular API call", 2),
        ("Premium feature", 10),
        ("Heavy computation", 25),
        ("Another API call", 1),
        ("Batch processing", 50),
    ];

    for (operation, cost) in requests {
        let decision = limiter
            .check_and_consume("premium_user", cost, Some(config.clone()))
            .await?;

        match (decision.allowed, &decision.denial_reason) {
            (true, None) => {
                println!("✓ {} (cost: {}) - Allowed", operation, cost);
                println!(
                    "  Rate: {:.1} tokens remaining",
                    decision.rate_limit.remaining_tokens
                );
                if let Some(quota) = &decision.quota {
                    if let Some(status) = &quota.status {
                        if let Some(daily) = &status.daily {
                            println!("  Quota: {}/{} used", daily.used, daily.limit);
                        }
                    }
                }
            }
            (false, Some(DenialReason::RateLimit)) => {
                println!("✗ {} (cost: {}) - Rate limited", operation, cost);
                if let Some(retry_ms) = decision.retry_after_ms() {
                    println!("  Retry after: {}ms", retry_ms);
                }
            }
            (false, Some(DenialReason::DailyQuota)) => {
                println!("✗ {} (cost: {}) - Daily quota exceeded", operation, cost);
            }
            (false, Some(DenialReason::MonthlyQuota)) => {
                println!("✗ {} (cost: {}) - Monthly quota exceeded", operation, cost);
            }
            _ => println!("? {} (cost: {}) - Unknown status", operation, cost),
        }

        // Small delay to simulate time passing
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    println!();
    Ok(())
}

/// Example 4: API Gateway usage pattern
async fn api_gateway_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("4. API Gateway Usage Pattern");
    println!("============================");

    let store = MemoryStore::new();
    let limiter = UnifiedLimiter::with_store(store);

    // Configure different tiers
    let free_tier = RateLimitConfig::new(1, 5, 60) // 1 req/sec, burst 5
        .with_quota(QuotaConfig::daily_only(100)); // 100 requests/day

    let premium_tier = RateLimitConfig::new(10, 50, 60) // 10 req/sec, burst 50
        .with_quota(QuotaConfig::both(5000, 150000)); // 5k/day, 150k/month

    // Set up different API keys
    limiter.set_config("free_api_key", free_tier, None).await?;
    limiter
        .set_config("premium_api_key", premium_tier, None)
        .await?;

    // Simulate API Gateway request handling
    let requests = [
        (
            "free_api_key",
            "GET /users",
            calculate_request_cost("GET", "/users", None),
        ),
        (
            "premium_api_key",
            "POST /ai/generate",
            calculate_request_cost("POST", "/ai/generate", Some(1024)),
        ),
        (
            "free_api_key",
            "GET /data/export",
            calculate_request_cost("GET", "/data/export", Some(10240)),
        ),
        (
            "premium_api_key",
            "PUT /files/upload",
            calculate_request_cost("PUT", "/files/upload", Some(2048)),
        ),
    ];

    for (api_key, endpoint, cost) in requests {
        println!("Processing: {} {} (cost: {})", api_key, endpoint, cost);

        let decision = limiter.check_and_consume(api_key, cost, None).await?;

        if decision.allowed {
            println!("✓ Request allowed - processing...");

            // Simulate request processing
            process_api_request(endpoint).await;

            // Show current status
            let status = limiter.get_status(api_key, None).await?;
            println!(
                "  Status: {:.1} tokens, quota: {:?}",
                status.rate_limit.remaining_tokens,
                status
                    .quota
                    .as_ref()
                    .map(|q| q.daily.as_ref().map(|d| format!("{}/{}", d.used, d.limit)))
                    .flatten()
                    .unwrap_or_else(|| "No quota".to_string())
            );
        } else {
            let reason = match decision.denial_reason {
                Some(DenialReason::RateLimit) => "rate limited",
                Some(DenialReason::DailyQuota) => "daily quota exceeded",
                Some(DenialReason::MonthlyQuota) => "monthly quota exceeded",
                None => "unknown reason",
            };
            println!("✗ Request denied: {}", reason);

            if let Some(retry_ms) = decision.retry_after_ms() {
                println!("  Client should retry after: {}ms", retry_ms);
            }
        }

        println!();
    }

    Ok(())
}

/// Calculate request cost based on endpoint and payload size
fn calculate_request_cost(method: &str, path: &str, payload_size: Option<usize>) -> u64 {
    let base_cost = match method {
        "GET" => 1,
        "POST" | "PUT" => 2,
        "DELETE" => 1,
        _ => 1,
    };

    let path_multiplier = if path.contains("/ai/") {
        50 // AI operations are expensive
    } else if path.contains("/export") || path.contains("/bulk") {
        10 // Data operations are moderately expensive
    } else if path.contains("/upload") {
        5 // File operations
    } else {
        1 // Regular operations
    };

    let size_cost = payload_size
        .map(|size| (size / 1024).max(1) as u64) // 1 cost per KB
        .unwrap_or(0);

    base_cost * path_multiplier + size_cost
}

/// Simulate API request processing
async fn process_api_request(endpoint: &str) {
    let processing_time = if endpoint.contains("/ai/") {
        Duration::from_millis(1000) // AI takes longer
    } else if endpoint.contains("/export") {
        Duration::from_millis(500) // Data export takes time
    } else {
        Duration::from_millis(50) // Regular requests are fast
    };

    tokio::time::sleep(processing_time).await;
    println!("  Request processed in {:?}", processing_time);
}
