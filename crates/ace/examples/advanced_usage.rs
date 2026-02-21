use ace::{
    Condition, Expression, Operator, Policy, PolicyAction, PolicyEngine, Value, context_with,
};
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Advanced ACE Policy Engine Usage");
    println!("=====================================\n");

    // Example 1: Complex nested conditions
    demonstrate_nested_conditions()?;

    // Example 2: Dynamic policy loading from multiple sources
    demonstrate_dynamic_loading()?;

    // Example 3: Policy conflicts and resolution
    demonstrate_policy_conflicts()?;

    // Example 4: Performance testing with many policies
    demonstrate_performance_testing()?;

    // Example 5: Custom policy builders
    demonstrate_policy_builders()?;

    // Example 6: Role-based access with hierarchies
    demonstrate_role_hierarchies()?;

    // Example 7: Time-based and contextual access
    demonstrate_contextual_access()?;

    Ok(())
}

fn demonstrate_nested_conditions() -> Result<(), Box<dyn std::error::Error>> {
    println!("📋 1. Complex Nested Conditions");
    println!("-------------------------------");

    let mut engine = PolicyEngine::new();

    let complex_policies = r#"
        ALLOW user FOR "sensitive_data" WHEN user.role == "admin" AND user.department == "security" AND user.clearance_level == "top_secret";
        ALLOW user FOR "financial_reports" WHEN (user.role == "cfo" OR user.role == "finance_manager") AND user.active == true;
        DENY user FOR "hr_records" WHEN user.department == "external" OR user.contractor == true;
        ALLOW user FOR "development_tools" WHEN user.role == "developer" AND (user.team == "backend" OR user.team == "frontend") AND user.experience_years >= 2;
    "#;

    engine.parse_file(complex_policies)?;

    // Test security admin
    let security_admin = context_with(vec![
        ("user.role", Value::String("admin".to_string())),
        ("user.department", Value::String("security".to_string())),
        (
            "user.clearance_level",
            Value::String("top_secret".to_string()),
        ),
        ("user.active", Value::Boolean(true)),
    ]);

    test_with_details(
        &engine,
        "user",
        "sensitive_data",
        &security_admin,
        "Security Admin",
    );

    // Test CFO
    let cfo = context_with(vec![
        ("user.role", Value::String("cfo".to_string())),
        ("user.active", Value::Boolean(true)),
    ]);

    test_with_details(&engine, "user", "financial_reports", &cfo, "CFO");

    // Test contractor
    let contractor = context_with(vec![
        ("user.role", Value::String("developer".to_string())),
        ("user.contractor", Value::Boolean(true)),
        ("user.department", Value::String("engineering".to_string())),
    ]);

    test_with_details(&engine, "user", "hr_records", &contractor, "Contractor");

    println!();
    Ok(())
}

fn demonstrate_dynamic_loading() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔄 2. Dynamic Policy Loading");
    println!("----------------------------");

    let mut engine = PolicyEngine::new();

    // Simulate loading from multiple configuration sources
    let base_policies = r#"
        ALLOW user FOR "login" WHEN user.active == true;
        DENY user FOR "admin_access" WHEN user.suspended == true;
    "#;

    let feature_policies = r#"
        ALLOW user FOR "beta_features" WHEN user.beta_tester == true;
        ALLOW user FOR "premium_features" WHEN user.subscription == "premium";
    "#;

    let api_policies = r#"
        ALLOW api_key FOR "public_api" WHEN api_key.valid == true;
        ALLOW api_key FOR "private_api" WHEN api_key.valid == true AND api_key.tier == "enterprise";
        DENY api_key FOR "admin_api" WHEN api_key.rate_limited == true;
    "#;

    // Load policies from multiple sources
    engine.load_from_sources(vec![base_policies, feature_policies, api_policies])?;

    println!(
        "Loaded {} policies from multiple sources",
        engine.get_policies().len()
    );

    // Test different scenarios
    let premium_user = context_with(vec![
        ("user.active", Value::Boolean(true)),
        ("user.subscription", Value::String("premium".to_string())),
        ("user.beta_tester", Value::Boolean(true)),
    ]);

    test_access(
        &engine,
        "user",
        "login",
        &premium_user,
        "Premium user login",
    );
    test_access(
        &engine,
        "user",
        "premium_features",
        &premium_user,
        "Premium user features",
    );
    test_access(
        &engine,
        "user",
        "beta_features",
        &premium_user,
        "Premium user beta access",
    );

    // Test API access
    let enterprise_api = context_with(vec![
        ("api_key.valid", Value::Boolean(true)),
        ("api_key.tier", Value::String("enterprise".to_string())),
        ("api_key.rate_limited", Value::Boolean(false)),
    ]);

    test_access(
        &engine,
        "api_key",
        "private_api",
        &enterprise_api,
        "Enterprise API access",
    );
    test_access(
        &engine,
        "api_key",
        "admin_api",
        &enterprise_api,
        "Enterprise admin API",
    );

    println!();
    Ok(())
}

fn demonstrate_policy_conflicts() -> Result<(), Box<dyn std::error::Error>> {
    println!("⚔️  3. Policy Conflicts and Resolution");
    println!("------------------------------------");

    let mut engine = PolicyEngine::new();

    let conflicting_policies = r#"
        ALLOW user FOR "disputed_resource" WHEN user.role == "manager";
        DENY user FOR "disputed_resource" WHEN user.on_probation == true;

        ALLOW user FOR "shared_resource" WHEN user.department == "engineering";
        ALLOW user FOR "shared_resource" WHEN user.department == "product";
        ALLOW user FOR "shared_resource" WHEN user.role == "admin";
    "#;

    engine.parse_file(conflicting_policies)?;

    // Test conflict where deny should win
    let probation_manager = context_with(vec![
        ("user.role", Value::String("manager".to_string())),
        ("user.on_probation", Value::Boolean(true)),
    ]);

    let result = engine.evaluate_with_details("user", "disputed_resource", &probation_manager);
    println!("Manager on probation accessing disputed resource:");
    println!(
        "  Decision: {}",
        if result.is_allowed() { "ALLOW" } else { "DENY" }
    );
    println!(
        "  Explanation: {} allow policies, {} deny policies",
        result.allow_count, result.deny_count
    );
    println!("  Rule: DENY takes precedence over ALLOW");

    // Test multiple allows
    let engineer = context_with(vec![
        ("user.department", Value::String("engineering".to_string())),
        ("user.role", Value::String("developer".to_string())),
    ]);

    let result = engine.evaluate_with_details("user", "shared_resource", &engineer);
    println!("\nEngineer accessing shared resource:");
    println!(
        "  Decision: {}",
        if result.is_allowed() { "ALLOW" } else { "DENY" }
    );
    println!("  Applied policies: {:?}", result.applied_policies);

    println!();
    Ok(())
}

fn demonstrate_performance_testing() -> Result<(), Box<dyn std::error::Error>> {
    println!("⚡ 4. Performance Testing");
    println!("------------------------");

    let mut engine = PolicyEngine::new();

    // Generate many policies programmatically
    for i in 0..100 {
        let policy = Policy::new(
            if i % 2 == 0 {
                PolicyAction::Allow
            } else {
                PolicyAction::Deny
            },
            "user".to_string(),
            format!("resource_{}", i),
        )
        .with_condition(Condition::Expression(Expression {
            left: "user.id".to_string(),
            operator: Operator::Equal,
            right: Value::Number(i as i64),
        }));
        engine.add_policy(policy);
    }

    println!(
        "Generated {} policies for performance testing",
        engine.get_policies().len()
    );

    // Time evaluation
    let start = std::time::Instant::now();
    let iterations = 1000;

    for i in 0..iterations {
        let context = context_with(vec![("user.id", Value::Number((i % 100) as i64))]);

        let _ = engine.evaluate("user", &format!("resource_{}", i % 100), &context);
    }

    let duration = start.elapsed();
    println!("Performed {} evaluations in {:?}", iterations, duration);
    println!("Average: {:?} per evaluation", duration / iterations);

    println!();
    Ok(())
}

fn demonstrate_policy_builders() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔨 5. Custom Policy Builders");
    println!("----------------------------");

    let mut engine = PolicyEngine::new();

    // Build policies programmatically with a builder pattern
    let admin_policy = Policy::new(
        PolicyAction::Allow,
        "user".to_string(),
        "admin_panel".to_string(),
    )
    .with_condition(Condition::And(
        Box::new(Condition::Expression(Expression {
            left: "user.role".to_string(),
            operator: Operator::Equal,
            right: Value::String("admin".to_string()),
        })),
        Box::new(Condition::Expression(Expression {
            left: "user.mfa_enabled".to_string(),
            operator: Operator::Equal,
            right: Value::Boolean(true),
        })),
    ));

    let conditional_access_policy = Policy::new(
        PolicyAction::Allow,
        "user".to_string(),
        "conditional_resource".to_string(),
    )
    .with_condition(Condition::Or(
        Box::new(Condition::And(
            Box::new(Condition::Expression(Expression {
                left: "user.location".to_string(),
                operator: Operator::Equal,
                right: Value::String("office".to_string()),
            })),
            Box::new(Condition::Expression(Expression {
                left: "user.device_trusted".to_string(),
                operator: Operator::Equal,
                right: Value::Boolean(true),
            })),
        )),
        Box::new(Condition::Expression(Expression {
            left: "user.emergency_access".to_string(),
            operator: Operator::Equal,
            right: Value::Boolean(true),
        })),
    ));

    engine.add_policy(admin_policy);
    engine.add_policy(conditional_access_policy);

    // Test admin with MFA
    let admin_with_mfa = context_with(vec![
        ("user.role", Value::String("admin".to_string())),
        ("user.mfa_enabled", Value::Boolean(true)),
    ]);

    test_access(
        &engine,
        "user",
        "admin_panel",
        &admin_with_mfa,
        "Admin with MFA",
    );

    // Test conditional access from office
    let office_user = context_with(vec![
        ("user.location", Value::String("office".to_string())),
        ("user.device_trusted", Value::Boolean(true)),
        ("user.emergency_access", Value::Boolean(false)),
    ]);

    test_access(
        &engine,
        "user",
        "conditional_resource",
        &office_user,
        "Office user with trusted device",
    );

    // Test emergency access
    let emergency_user = context_with(vec![
        ("user.location", Value::String("home".to_string())),
        ("user.device_trusted", Value::Boolean(false)),
        ("user.emergency_access", Value::Boolean(true)),
    ]);

    test_access(
        &engine,
        "user",
        "conditional_resource",
        &emergency_user,
        "Emergency access user",
    );

    println!();
    Ok(())
}

fn demonstrate_role_hierarchies() -> Result<(), Box<dyn std::error::Error>> {
    println!("👑 6. Role-Based Access with Hierarchies");
    println!("----------------------------------------");

    let mut engine = PolicyEngine::new();

    let hierarchy_policies = r#"
        ALLOW user FOR "basic_access" WHEN user.role == "intern" OR user.role == "employee" OR user.role == "manager" OR user.role == "admin";
        ALLOW user FOR "employee_data" WHEN user.role == "manager" OR user.role == "admin";
        ALLOW user FOR "financial_data" WHEN user.role == "admin";
        ALLOW user FOR "system_config" WHEN user.role == "admin" AND user.system_admin == true;

        ALLOW user FOR "hr_data" WHEN user.department == "hr" AND (user.role == "manager" OR user.role == "admin");
        ALLOW user FOR "engineering_tools" WHEN user.department == "engineering";
    "#;

    engine.parse_file(hierarchy_policies)?;

    // Test different role levels
    let roles = vec![
        ("intern", "Intern"),
        ("employee", "Employee"),
        ("manager", "Manager"),
        ("admin", "Admin"),
    ];

    let resources = vec![
        ("basic_access", "Basic Access"),
        ("employee_data", "Employee Data"),
        ("financial_data", "Financial Data"),
        ("system_config", "System Config"),
    ];

    println!("Role Hierarchy Access Matrix:");
    println!(
        "{:<12} {:<15} {:<15} {:<15} {:<15}",
        "Role", "Basic Access", "Employee Data", "Financial Data", "System Config"
    );
    println!("{}", "-".repeat(75));

    for (role, role_name) in roles {
        let context = context_with(vec![
            ("user.role", Value::String(role.to_string())),
            ("user.system_admin", Value::Boolean(role == "admin")),
            ("user.department", Value::String("engineering".to_string())),
        ]);

        print!("{:<12}", role_name);

        for (resource, _) in &resources {
            let allowed = engine.evaluate("user", resource, &context);
            print!(" {:<15}", if allowed { "✅ ALLOW" } else { "❌ DENY" });
        }
        println!();
    }

    // Test HR department access
    println!("\nHR Department Access:");
    let hr_manager = context_with(vec![
        ("user.role", Value::String("manager".to_string())),
        ("user.department", Value::String("hr".to_string())),
    ]);

    test_access(
        &engine,
        "user",
        "hr_data",
        &hr_manager,
        "HR Manager accessing HR data",
    );
    test_access(
        &engine,
        "user",
        "engineering_tools",
        &hr_manager,
        "HR Manager accessing engineering tools",
    );

    println!();
    Ok(())
}

fn demonstrate_contextual_access() -> Result<(), Box<dyn std::error::Error>> {
    println!("🕐 7. Time-based and Contextual Access");
    println!("-------------------------------------");

    let mut engine = PolicyEngine::new();

    let contextual_policies = r#"
        ALLOW user FOR "work_resources" WHEN user.time_of_day == "business_hours" AND user.location == "office";
        ALLOW user FOR "emergency_access" WHEN user.emergency_override == true;
        DENY user FOR "restricted_hours" WHEN user.time_of_day == "after_hours" AND user.location != "office";

        ALLOW user FOR "mobile_access" WHEN user.device_type == "mobile" AND user.device_registered == true;
        DENY user FOR "sensitive_mobile" WHEN user.device_type == "mobile" AND user.device_encrypted == false;

        ALLOW user FOR "geo_restricted" WHEN user.country == "US" OR user.country == "CA";
        DENY user FOR "ip_blocked" WHEN user.ip_suspicious == true;
    "#;

    engine.parse_file(contextual_policies)?;

    println!("Testing contextual access scenarios:");

    // Business hours office access
    let business_context = context_with(vec![
        (
            "user.time_of_day",
            Value::String("business_hours".to_string()),
        ),
        ("user.location", Value::String("office".to_string())),
    ]);

    test_access(
        &engine,
        "user",
        "work_resources",
        &business_context,
        "Business hours office access",
    );

    // After hours remote access (should be denied)
    let after_hours_context = context_with(vec![
        ("user.time_of_day", Value::String("after_hours".to_string())),
        ("user.location", Value::String("home".to_string())),
    ]);

    test_access(
        &engine,
        "user",
        "restricted_hours",
        &after_hours_context,
        "After hours remote access",
    );

    // Emergency override
    let emergency_context = context_with(vec![
        ("user.time_of_day", Value::String("after_hours".to_string())),
        ("user.location", Value::String("home".to_string())),
        ("user.emergency_override", Value::Boolean(true)),
    ]);

    test_access(
        &engine,
        "user",
        "emergency_access",
        &emergency_context,
        "Emergency override access",
    );

    // Mobile device access
    let mobile_context = context_with(vec![
        ("user.device_type", Value::String("mobile".to_string())),
        ("user.device_registered", Value::Boolean(true)),
        ("user.device_encrypted", Value::Boolean(true)),
    ]);

    test_access(
        &engine,
        "user",
        "mobile_access",
        &mobile_context,
        "Registered mobile access",
    );
    test_access(
        &engine,
        "user",
        "sensitive_mobile",
        &mobile_context,
        "Mobile sensitive data access",
    );

    // Unencrypted mobile (should be denied for sensitive)
    let unencrypted_mobile = context_with(vec![
        ("user.device_type", Value::String("mobile".to_string())),
        ("user.device_registered", Value::Boolean(true)),
        ("user.device_encrypted", Value::Boolean(false)),
    ]);

    test_access(
        &engine,
        "user",
        "sensitive_mobile",
        &unencrypted_mobile,
        "Unencrypted mobile sensitive access",
    );

    // Geographic restrictions
    let us_user = context_with(vec![
        ("user.country", Value::String("US".to_string())),
        ("user.ip_suspicious", Value::Boolean(false)),
    ]);

    test_access(
        &engine,
        "user",
        "geo_restricted",
        &us_user,
        "US user geographic access",
    );

    let suspicious_ip = context_with(vec![
        ("user.country", Value::String("US".to_string())),
        ("user.ip_suspicious", Value::Boolean(true)),
    ]);

    test_access(
        &engine,
        "user",
        "ip_blocked",
        &suspicious_ip,
        "Suspicious IP access",
    );

    println!();
    Ok(())
}

fn test_access(
    engine: &PolicyEngine,
    subject: &str,
    resource: &str,
    context: &HashMap<String, Value>,
    description: &str,
) {
    let allowed = engine.evaluate(subject, resource, context);
    let result = if allowed { "✅ ALLOWED" } else { "❌ DENIED" };
    println!("  {}: {}", result, description);
}

fn test_with_details(
    engine: &PolicyEngine,
    subject: &str,
    resource: &str,
    context: &HashMap<String, Value>,
    description: &str,
) {
    let result = engine.evaluate_with_details(subject, resource, context);
    let status = if result.is_allowed() {
        "✅ ALLOWED"
    } else {
        "❌ DENIED"
    };
    println!("  {}: {}", status, description);
    println!(
        "    📊 Details: {} matched, {} applied ({} allow, {} deny)",
        result.matched_policies.len(),
        result.applied_policies.len(),
        result.allow_count,
        result.deny_count
    );
}
