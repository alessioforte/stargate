use ace::{context_with, ContextBuilder, PolicyEngine, ResourceAction, Value};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎯 Practical ACE Usage - Real World Scenarios");
    println!("==============================================\n");

    let mut engine = PolicyEngine::new();

    // Original syntax from your file, enhanced with new features
    let practical_policies = r#"
        // Original basic policies with enhancements
        // ALLOW user FOR "feature1" WHEN user.role == "admin" OR user.role == "editor";
        // DENY user FOR "feature2" WHEN user.role == "guest";
        // ALLOW api_key FOR "feature3" WHEN api_key.valid == true AND api_key.scope == "read";
        // DENY api_key FOR "feature4" WHEN api_key.valid == false OR api_key.scope != "write";

        // Extended with resource actions
        // ALLOW user FOR "user_management:READ" WHEN user.role == "admin" OR user.role == "manager";
        // ALLOW user FOR "user_management:WRITE" WHEN user.role == "admin";
        // ALLOW user FOR "user_management:DELETE" WHEN user.role == "admin" AND user.mfa_enabled == true;

        // Age-based content access
        // ALLOW user FOR "mature_content" WHEN user.age >= 18 AND user.verified == true;
        // DENY user FOR "mature_content" WHEN user.parental_controls == true;

        // Location and time-based policies
        // ALLOW user FOR "office_printer:*" WHEN location == "office" OR (location == "remote" AND user.vpn_connected == true);
        // DENY user FOR "after_hours_systems" WHEN time.of_day == "night" AND NOT user.emergency_access == true;

        // Security clearance with NOT conditions
        // ALLOW user FOR "classified_docs:READ" WHEN user.clearance_level >= 3 AND NOT user.under_investigation == true;
        // ALLOW user FOR "classified_docs:WRITE" WHEN user.clearance_level >= 4 AND user.dual_approval == true;

        // Score-based premium features
        // ALLOW user FOR "premium_api:*" WHEN user.credit_score >= 750 OR user.premium_member == true;
        // ALLOW user FOR "high_limit_transactions" WHEN user.account_balance > 10000 AND user.transaction_history_months >= 12;

        // Device security policies
        // ALLOW user FOR "mobile_app:READ" WHEN device.type == "mobile" AND device.app_version >= 2.0;
        // DENY user FOR "mobile_app:WRITE" WHEN device.type == "mobile" AND device.rooted == true;

        // Rate limiting and abuse prevention
        // DENY api_key FOR "bulk_operations:*" WHEN api_key.requests_per_hour > 1000;
        // ALLOW api_key FOR "batch_processing" WHEN api_key.tier == "enterprise" AND api_key.concurrent_jobs <= 5;
    "#;

    engine.parse_file(practical_policies)?;
    println!(
        "Loaded {} practical policies\n",
        engine.get_policies().len()
    );

    // Demonstrate original scenarios enhanced
    demonstrate_original_enhanced(&engine)?;

    // Demonstrate resource actions in practice
    demonstrate_user_management(&engine)?;

    // Demonstrate age and verification scenarios
    demonstrate_content_access(&engine)?;

    // Demonstrate location and time policies
    demonstrate_contextual_access(&engine)?;

    // Demonstrate security clearance
    demonstrate_security_clearance(&engine)?;

    // Demonstrate financial/scoring scenarios
    demonstrate_financial_access(&engine)?;

    // Demonstrate device security
    demonstrate_device_policies(&engine)?;

    // Demonstrate API rate limiting
    demonstrate_api_policies(&engine)?;

    Ok(())
}

fn demonstrate_original_enhanced(engine: &PolicyEngine) -> Result<(), Box<dyn std::error::Error>> {
    println!("📝 Original Policies Enhanced");
    println!("-----------------------------");

    // Test the original policy patterns
    let admin_context = context_with(vec![("user.role", Value::String("admin".to_string()))]);

    let editor_context = context_with(vec![("user.role", Value::String("editor".to_string()))]);

    let guest_context = context_with(vec![("user.role", Value::String("guest".to_string()))]);

    println!("User Access Tests:");
    test_access(
        &engine,
        "user",
        "feature1",
        &admin_context,
        "Admin → feature1",
    );
    test_access(
        &engine,
        "user",
        "feature1",
        &editor_context,
        "Editor → feature1",
    );
    test_access(
        &engine,
        "user",
        "feature1",
        &guest_context,
        "Guest → feature1",
    );

    test_access(
        &engine,
        "user",
        "feature2",
        &admin_context,
        "Admin → feature2",
    );
    test_access(
        &engine,
        "user",
        "feature2",
        &guest_context,
        "Guest → feature2",
    );

    // API key tests
    let valid_read_api = context_with(vec![
        ("api_key.valid", Value::Boolean(true)),
        ("api_key.scope", Value::String("read".to_string())),
    ]);

    let invalid_api = context_with(vec![
        ("api_key.valid", Value::Boolean(false)),
        ("api_key.scope", Value::String("read".to_string())),
    ]);

    let wrong_scope_api = context_with(vec![
        ("api_key.valid", Value::Boolean(true)),
        ("api_key.scope", Value::String("read".to_string())),
    ]);

    println!("\nAPI Key Access Tests:");
    test_access(
        &engine,
        "api_key",
        "feature3",
        &valid_read_api,
        "Valid read API → feature3",
    );
    test_access(
        &engine,
        "api_key",
        "feature4",
        &wrong_scope_api,
        "Read API → write feature4",
    );
    test_access(
        &engine,
        "api_key",
        "feature3",
        &invalid_api,
        "Invalid API → feature3",
    );

    println!();
    Ok(())
}

fn demonstrate_user_management(engine: &PolicyEngine) -> Result<(), Box<dyn std::error::Error>> {
    println!("👥 User Management with Resource Actions");
    println!("----------------------------------------");

    let roles = vec![("admin", true), ("manager", false), ("employee", false)];

    let actions = vec![
        (ResourceAction::Read, "READ"),
        (ResourceAction::Write, "WRITE"),
        (ResourceAction::Delete, "DELETE"),
    ];

    println!(
        "{:<12} {:<8} {:<8} {:<8}",
        "Role", "READ", "WRITE", "DELETE"
    );
    println!("{}", "-".repeat(40));

    for (role, mfa_enabled) in roles {
        let context = context_with(vec![
            ("user.role", Value::String(role.to_string())),
            ("user.mfa_enabled", Value::Boolean(mfa_enabled)),
        ]);

        print!("{:<12}", role);
        for (action, _) in &actions {
            let allowed = engine.evaluate_with_action("user", "user_management", action, &context);
            print!(" {:<8}", if allowed { "✅" } else { "❌" });
        }
        println!();
    }

    println!();
    Ok(())
}

fn demonstrate_content_access(engine: &PolicyEngine) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔞 Age-Based Content Access");
    println!("----------------------------");

    let scenarios = vec![
        ("Minor (16) - verified", 16, true, false),
        ("Adult (25) - verified", 25, true, false),
        ("Adult (25) - unverified", 25, false, false),
        ("Adult (25) - parental controls", 25, true, true),
    ];

    for (description, age, verified, parental_controls) in scenarios {
        let context = context_with(vec![
            ("user.age", Value::Number(age)),
            ("user.verified", Value::Boolean(verified)),
            ("user.parental_controls", Value::Boolean(parental_controls)),
        ]);

        let mature_access = engine.evaluate("user", "mature_content", &context);
        println!("  {}: {}", description, format_result(mature_access));
    }

    println!();
    Ok(())
}

fn demonstrate_contextual_access(engine: &PolicyEngine) -> Result<(), Box<dyn std::error::Error>> {
    println!("📍 Location and Time-Based Access");
    println!("----------------------------------");

    // Office printer access scenarios
    let printer_scenarios = vec![
        ("Office location", "office", false),
        ("Remote with VPN", "remote", true),
        ("Remote without VPN", "remote", false),
        ("Home location", "home", false),
    ];

    println!("Office Printer Access:");
    for (description, location, vpn_connected) in printer_scenarios {
        let context = context_with(vec![
            ("location", Value::String(location.to_string())),
            ("user.vpn_connected", Value::Boolean(vpn_connected)),
        ]);

        let printer_access =
            engine.evaluate_with_action("user", "office_printer", &ResourceAction::Any, &context);
        println!("  {}: {}", description, format_result(printer_access));
    }

    // After hours system access
    println!("\nAfter Hours System Access:");
    let after_hours_scenarios = vec![
        ("Day time - regular user", "day", false),
        ("Night - emergency access", "night", true),
        ("Night - regular user", "night", false),
    ];

    for (description, time_of_day, emergency_access) in after_hours_scenarios {
        let context = context_with(vec![
            ("time.of_day", Value::String(time_of_day.to_string())),
            ("user.emergency_access", Value::Boolean(emergency_access)),
        ]);

        let system_access = engine.evaluate("user", "after_hours_systems", &context);
        println!("  {}: {}", description, format_result(!system_access)); // Inverted because it's DENY
    }

    println!();
    Ok(())
}

fn demonstrate_security_clearance(engine: &PolicyEngine) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔐 Security Clearance Access");
    println!("-----------------------------");

    let clearance_scenarios = vec![
        ("Basic clearance", 2, false, false),
        ("Secret clearance", 3, false, false),
        ("Secret under investigation", 3, true, false),
        ("Top secret", 4, false, false),
        ("Top secret with dual approval", 4, false, true),
        ("Top secret under investigation", 4, true, true),
    ];

    println!("{:<30} {:<8} {:<8}", "Scenario", "READ", "WRITE");
    println!("{}", "-".repeat(50));

    for (description, clearance_level, under_investigation, dual_approval) in clearance_scenarios {
        let context = context_with(vec![
            ("user.clearance_level", Value::Number(clearance_level)),
            (
                "user.under_investigation",
                Value::Boolean(under_investigation),
            ),
            ("user.dual_approval", Value::Boolean(dual_approval)),
        ]);

        let read_access =
            engine.evaluate_with_action("user", "classified_docs", &ResourceAction::Read, &context);
        let write_access = engine.evaluate_with_action(
            "user",
            "classified_docs",
            &ResourceAction::Write,
            &context,
        );

        println!(
            "{:<30} {:<8} {:<8}",
            description,
            if read_access { "✅" } else { "❌" },
            if write_access { "✅" } else { "❌" }
        );
    }

    println!();
    Ok(())
}

fn demonstrate_financial_access(engine: &PolicyEngine) -> Result<(), Box<dyn std::error::Error>> {
    println!("💰 Financial and Score-Based Access");
    println!("-----------------------------------");

    let financial_scenarios = vec![
        ("Low credit score", 650, false, 5000.0, 6),
        ("Good credit, not premium", 750, false, 15000.0, 18),
        ("Excellent credit, premium", 800, true, 25000.0, 24),
        ("Premium but low balance", 750, true, 2000.0, 30),
    ];

    println!(
        "{:<25} {:<12} {:<12}",
        "Scenario", "Premium API", "High Limit"
    );
    println!("{}", "-".repeat(50));

    for (description, credit_score, premium_member, balance, history_months) in financial_scenarios
    {
        let context = context_with(vec![
            ("user.credit_score", Value::Number(credit_score)),
            ("user.premium_member", Value::Boolean(premium_member)),
            ("user.account_balance", Value::Float(balance)),
            (
                "user.transaction_history_months",
                Value::Number(history_months),
            ),
        ]);

        let premium_api =
            engine.evaluate_with_action("user", "premium_api", &ResourceAction::Any, &context);
        let high_limit = engine.evaluate("user", "high_limit_transactions", &context);

        println!(
            "{:<25} {:<12} {:<12}",
            description,
            if premium_api { "✅" } else { "❌" },
            if high_limit { "✅" } else { "❌" }
        );
    }

    println!();
    Ok(())
}

fn demonstrate_device_policies(engine: &PolicyEngine) -> Result<(), Box<dyn std::error::Error>> {
    println!("📱 Device Security Policies");
    println!("---------------------------");

    let device_scenarios = vec![
        ("Modern mobile app", "mobile", 2.1, false),
        ("Outdated mobile app", "mobile", 1.9, false),
        ("Rooted device", "mobile", 2.2, true),
        ("Desktop browser", "desktop", 0.0, false),
    ];

    for (description, device_type, app_version, rooted) in device_scenarios {
        let context = ContextBuilder::new()
            .device_type(device_type)
            .custom("device.app_version", Value::Float(app_version))
            .custom("device.rooted", Value::Boolean(rooted))
            .build();

        let read_access =
            engine.evaluate_with_action("user", "mobile_app", &ResourceAction::Read, &context);
        let write_access =
            engine.evaluate_with_action("user", "mobile_app", &ResourceAction::Write, &context);

        println!(
            "  {}: READ: {}, WRITE: {}",
            description,
            format_result(read_access),
            format_result(write_access)
        );
    }

    println!();
    Ok(())
}

fn demonstrate_api_policies(engine: &PolicyEngine) -> Result<(), Box<dyn std::error::Error>> {
    println!("🚦 API Rate Limiting and Tiers");
    println!("-------------------------------");

    let api_scenarios = vec![
        ("Basic tier - normal usage", "basic", 500, 2),
        ("Basic tier - high usage", "basic", 1500, 3),
        ("Enterprise - normal usage", "enterprise", 800, 3),
        ("Enterprise - high usage", "enterprise", 1200, 6),
        ("Enterprise - max concurrent", "enterprise", 900, 5),
    ];

    for (description, tier, requests_per_hour, concurrent_jobs) in api_scenarios {
        let context = context_with(vec![
            ("api_key.tier", Value::String(tier.to_string())),
            (
                "api_key.requests_per_hour",
                Value::Number(requests_per_hour),
            ),
            ("api_key.concurrent_jobs", Value::Number(concurrent_jobs)),
        ]);

        let bulk_ops = engine.evaluate_with_action(
            "api_key",
            "bulk_operations",
            &ResourceAction::Any,
            &context,
        );
        let batch_processing = engine.evaluate("api_key", "batch_processing", &context);

        println!(
            "  {}: Bulk Ops: {}, Batch: {}",
            description,
            format_result(!bulk_ops), // Inverted because it's DENY
            format_result(batch_processing)
        );
    }

    println!();
    Ok(())
}

fn test_access(
    engine: &PolicyEngine,
    subject: &str,
    resource: &str,
    context: &std::collections::HashMap<String, Value>,
    description: &str,
) {
    let allowed = engine.evaluate(subject, resource, context);
    println!("  {}: {}", description, format_result(allowed));
}

fn format_result(allowed: bool) -> String {
    if allowed {
        "✅ ALLOW".to_string()
    } else {
        "❌ DENY".to_string()
    }
}
