use ace::{ContextBuilder, PolicyEngine, ResourceAction, Value, context_with};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔧 Extended ACE Features Demonstration");
    println!("======================================\n");

    // Create a new policy engine
    let mut engine = PolicyEngine::new();

    // Define comprehensive policies using all new features
    let extended_policies = r#"
        ALLOW user FOR "adult_content" WHEN user.age >= 18;
        ALLOW user FOR "senior_discount" WHEN user.age > 65;
        DENY user FOR "teen_content" WHEN user.age > 17;

        ALLOW user FOR "database:READ" WHEN user.role == "analyst" OR user.role == "admin";
        ALLOW user FOR "database:WRITE" WHEN user.role == "editor" OR user.role == "admin";
        ALLOW user FOR "database:DELETE" WHEN user.role == "admin";
        DENY user FOR "database:DELETE" WHEN user.probation == true;

        ALLOW user FOR "secure_area" WHEN NOT user.suspended == true;
        ALLOW user FOR "vpn_access" WHEN NOT (user.location == "restricted" OR user.device_untrusted == true);

        ALLOW user FOR "after_hours_access" WHEN user.emergency_contact == true OR (user.role == "admin" AND time.of_day != "night");
        DENY user FOR "business_hours_only" WHEN time.of_day == "night" AND NOT user.on_call == true;

        ALLOW user FOR "mobile_banking:READ" WHEN device.type == "mobile" AND device.secure == true;
        DENY user FOR "mobile_banking:WRITE" WHEN device.type == "mobile" AND device.biometric_enabled != true;

        ALLOW user FOR "premium_features" WHEN user.score >= 95.5 AND user.subscription == "premium";
        ALLOW user FOR "beta_program" WHEN user.engagement_score > 80.0 OR user.beta_tester == true;

        ALLOW user FOR "classified:READ" WHEN security.level >= 3 AND user.clearance == "secret" AND NOT user.foreign_national == true;
        ALLOW user FOR "classified:WRITE" WHEN security.level >= 4 AND user.clearance == "top_secret";

        DENY user FOR "geo_restricted" WHEN request.country == "blocked" OR request.ip_suspicious == true;
        ALLOW user FOR "region_specific" WHEN request.country == "US" AND user.citizenship == "US";

        ALLOW user FOR "extended_session" WHEN user.session_duration <= 480 AND user.activity_recent == true;
        DENY user FOR "concurrent_limit" WHEN user.active_sessions > 3 AND user.role != "admin";
    "#;

    // Parse the policies
    engine.parse_file(extended_policies)?;
    println!(
        "Loaded {} policies with extended features\n",
        engine.get_policies().len()
    );

    // Demonstrate age-based access control
    demonstrate_age_based_access(&engine)?;

    // Demonstrate resource actions
    demonstrate_resource_actions(&engine)?;

    // Demonstrate NOT conditions
    demonstrate_not_conditions(&engine)?;

    // Demonstrate comparison operators
    demonstrate_comparison_operators(&engine)?;

    // Demonstrate context builder
    demonstrate_context_builder(&engine)?;

    // Demonstrate complex multi-condition scenarios
    demonstrate_complex_scenarios(&engine)?;

    // Demonstrate security and compliance scenarios
    demonstrate_security_scenarios(&engine)?;

    Ok(())
}

fn demonstrate_age_based_access(engine: &PolicyEngine) -> Result<(), Box<dyn std::error::Error>> {
    println!("👶 Age-Based Access Control");
    println!("---------------------------");

    let ages = vec![16, 18, 25, 66, 70];

    for age in ages {
        let context = context_with(vec![("user.age", Value::Number(age))]);

        let adult_content = engine.evaluate("user", "adult_content", &context);
        let senior_discount = engine.evaluate("user", "senior_discount", &context);
        let teen_content = engine.evaluate("user", "teen_content", &context);

        println!(
            "Age {}: Adult Content: {}, Senior Discount: {}, Teen Content: {}",
            age,
            format_result(adult_content),
            format_result(senior_discount),
            format_result(teen_content)
        );
    }

    println!();
    Ok(())
}

fn demonstrate_resource_actions(engine: &PolicyEngine) -> Result<(), Box<dyn std::error::Error>> {
    println!("🗄️  Resource Actions (Database Operations)");
    println!("------------------------------------------");

    let roles = vec![
        ("guest", "Guest User"),
        ("analyst", "Data Analyst"),
        ("editor", "Content Editor"),
        ("admin", "Administrator"),
    ];

    let actions = vec![
        (ResourceAction::Read, "READ"),
        (ResourceAction::Write, "WRITE"),
        (ResourceAction::Delete, "DELETE"),
    ];

    println!(
        "{:<15} {:<10} {:<10} {:<10}",
        "Role", "READ", "WRITE", "DELETE"
    );
    println!("{}", "-".repeat(45));

    for (role, role_name) in roles {
        let context = context_with(vec![
            ("user.role", Value::String(role.to_string())),
            ("user.probation", Value::Boolean(false)),
        ]);

        print!("{:<15}", role_name);

        for (action, _) in &actions {
            let allowed = engine.evaluate_with_action("user", "database", action, &context);
            print!(" {:<10}", format_result(allowed));
        }
        println!();
    }

    // Test admin on probation (should be denied DELETE)
    println!("\nAdmin on probation:");
    let admin_probation = context_with(vec![
        ("user.role", Value::String("admin".to_string())),
        ("user.probation", Value::Boolean(true)),
    ]);

    let delete_allowed = engine.evaluate_with_action(
        "user",
        "database",
        &ResourceAction::Delete,
        &admin_probation,
    );
    println!("  DELETE access: {}", format_result(delete_allowed));

    println!();
    Ok(())
}

fn demonstrate_not_conditions(engine: &PolicyEngine) -> Result<(), Box<dyn std::error::Error>> {
    println!("🚫 NOT Conditions");
    println!("-----------------");

    // Test NOT conditions for security access
    let test_cases = vec![
        ("Active user", false, false, false),
        ("Suspended user", true, false, false),
        ("User in restricted location", false, true, false),
        ("User with untrusted device", false, false, true),
        ("Suspended user in restricted location", true, true, true),
    ];

    for (description, suspended, restricted_location, untrusted_device) in test_cases {
        let context = context_with(vec![
            ("user.suspended", Value::Boolean(suspended)),
            (
                "user.location",
                Value::String(
                    if restricted_location {
                        "restricted"
                    } else {
                        "office"
                    }
                    .to_string(),
                ),
            ),
            ("user.device_untrusted", Value::Boolean(untrusted_device)),
        ]);

        let secure_area = engine.evaluate("user", "secure_area", &context);
        let vpn_access = engine.evaluate("user", "vpn_access", &context);

        println!(
            "{}: Secure Area: {}, VPN Access: {}",
            description,
            format_result(secure_area),
            format_result(vpn_access)
        );
    }

    println!();
    Ok(())
}

fn demonstrate_comparison_operators(
    engine: &PolicyEngine,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📊 Comparison Operators");
    println!("-----------------------");

    // Test score-based access
    let scores = vec![70.0, 85.5, 95.5, 98.0];

    for score in scores {
        let context = context_with(vec![
            ("user.score", Value::Float(score)),
            ("user.subscription", Value::String("premium".to_string())),
            ("user.engagement_score", Value::Float(score)),
            ("user.beta_tester", Value::Boolean(score > 90.0)),
        ]);

        let premium_features = engine.evaluate("user", "premium_features", &context);
        let beta_program = engine.evaluate("user", "beta_program", &context);

        println!(
            "Score {}: Premium Features: {}, Beta Program: {}",
            score,
            format_result(premium_features),
            format_result(beta_program)
        );
    }

    println!();
    Ok(())
}

fn demonstrate_context_builder(engine: &PolicyEngine) -> Result<(), Box<dyn std::error::Error>> {
    println!("🏗️  Context Builder");
    println!("-------------------");

    // Build rich context using the context builder
    let mobile_user_context = ContextBuilder::new()
        .user_role("customer")
        .device_type("mobile")
        .add("device.secure", Value::Boolean(true))
        .add("device.biometric_enabled", Value::Boolean(true))
        .build();

    let unsecure_mobile_context = ContextBuilder::new()
        .user_role("customer")
        .device_type("mobile")
        .add("device.secure", Value::Boolean(true))
        .add("device.biometric_enabled", Value::Boolean(false))
        .build();

    println!("Mobile Banking Access Tests:");

    let read_access = engine.evaluate_with_action(
        "user",
        "mobile_banking",
        &ResourceAction::Read,
        &mobile_user_context,
    );
    let write_access = engine.evaluate_with_action(
        "user",
        "mobile_banking",
        &ResourceAction::Write,
        &mobile_user_context,
    );

    println!(
        "  Secure Mobile - READ: {}, WRITE: {}",
        format_result(read_access),
        format_result(write_access)
    );

    let read_access_unsecure = engine.evaluate_with_action(
        "user",
        "mobile_banking",
        &ResourceAction::Read,
        &unsecure_mobile_context,
    );
    let write_access_unsecure = engine.evaluate_with_action(
        "user",
        "mobile_banking",
        &ResourceAction::Write,
        &unsecure_mobile_context,
    );

    println!(
        "  Unsecure Mobile - READ: {}, WRITE: {}",
        format_result(read_access_unsecure),
        format_result(write_access_unsecure)
    );

    // Build time-based context
    let business_hours_context = ContextBuilder::new()
        .user_role("employee")
        .time_of_day("day")
        .add("user.on_call", Value::Boolean(false))
        .build();

    let night_on_call_context = ContextBuilder::new()
        .user_role("employee")
        .time_of_day("night")
        .add("user.on_call", Value::Boolean(true))
        .build();

    let night_not_on_call_context = ContextBuilder::new()
        .user_role("employee")
        .time_of_day("night")
        .add("user.on_call", Value::Boolean(false))
        .build();

    println!("\nTime-Based Access Tests:");

    let business_day_access =
        engine.evaluate("user", "business_hours_only", &business_hours_context);
    let night_on_call_access =
        engine.evaluate("user", "business_hours_only", &night_on_call_context);
    let night_regular_access =
        engine.evaluate("user", "business_hours_only", &night_not_on_call_context);

    println!("  Business Hours: {}", format_result(business_day_access));
    println!("  Night (On-Call): {}", format_result(night_on_call_access));
    println!("  Night (Regular): {}", format_result(night_regular_access));

    println!();
    Ok(())
}

fn demonstrate_complex_scenarios(engine: &PolicyEngine) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔄 Complex Multi-Condition Scenarios");
    println!("------------------------------------");

    // Admin with various time and emergency scenarios
    let scenarios = vec![
        ("Admin during day", "admin", "day", false),
        ("Admin at night", "admin", "night", false),
        ("Emergency contact at night", "employee", "night", true),
        ("Regular employee at night", "employee", "night", false),
    ];

    for (description, role, time_of_day, emergency_contact) in scenarios {
        let context = context_with(vec![
            ("user.role", Value::String(role.to_string())),
            ("time.of_day", Value::String(time_of_day.to_string())),
            ("user.emergency_contact", Value::Boolean(emergency_contact)),
        ]);

        let after_hours_access = engine.evaluate("user", "after_hours_access", &context);

        println!(
            "{}: After Hours Access: {}",
            description,
            format_result(after_hours_access)
        );
    }

    // Session management scenarios
    println!("\nSession Management:");
    let session_scenarios = vec![
        ("Regular user - 2 sessions", "user", 2, 300),
        ("Regular user - 4 sessions", "user", 4, 300),
        ("Admin - 5 sessions", "admin", 5, 300),
        ("User - long session", "user", 2, 600),
    ];

    for (description, role, active_sessions, session_duration) in session_scenarios {
        let context = context_with(vec![
            ("user.role", Value::String(role.to_string())),
            ("user.active_sessions", Value::Number(active_sessions)),
            ("user.session_duration", Value::Number(session_duration)),
            ("user.activity_recent", Value::Boolean(true)),
        ]);

        let concurrent_limit = engine.evaluate("user", "concurrent_limit", &context);
        let extended_session = engine.evaluate("user", "extended_session", &context);

        println!(
            "{}: Concurrent OK: {}, Extended OK: {}",
            description,
            format_result(!concurrent_limit), // Inverted because it's a DENY policy
            format_result(extended_session)
        );
    }

    println!();
    Ok(())
}

fn demonstrate_security_scenarios(engine: &PolicyEngine) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔐 Security and Compliance Scenarios");
    println!("------------------------------------");

    // Classified data access scenarios
    let security_scenarios = vec![
        ("Low clearance", 2, "confidential", false),
        ("Secret clearance", 3, "secret", false),
        ("Secret + foreign national", 3, "secret", true),
        ("Top secret clearance", 4, "top_secret", false),
        ("High security + top secret", 5, "top_secret", false),
    ];

    println!("Classified Data Access:");
    for (description, security_level, clearance, foreign_national) in security_scenarios {
        let context = context_with(vec![
            ("security.level", Value::Number(security_level)),
            ("user.clearance", Value::String(clearance.to_string())),
            ("user.foreign_national", Value::Boolean(foreign_national)),
        ]);

        let read_access =
            engine.evaluate_with_action("user", "classified", &ResourceAction::Read, &context);
        let write_access =
            engine.evaluate_with_action("user", "classified", &ResourceAction::Write, &context);

        println!(
            "  {}: READ: {}, WRITE: {}",
            description,
            format_result(read_access),
            format_result(write_access)
        );
    }

    // Geographic and IP-based restrictions
    println!("\nGeographic Restrictions:");
    let geo_scenarios = vec![
        ("US citizen from US", "US", "US", false),
        ("US citizen from abroad", "CA", "US", false),
        ("Foreign user from US", "US", "UK", false),
        ("Blocked country", "blocked", "Unknown", false),
        ("Suspicious IP", "US", "US", true),
    ];

    for (description, country, citizenship, ip_suspicious) in geo_scenarios {
        let context = context_with(vec![
            ("request.country", Value::String(country.to_string())),
            ("user.citizenship", Value::String(citizenship.to_string())),
            ("request.ip_suspicious", Value::Boolean(ip_suspicious)),
        ]);

        let geo_restricted = engine.evaluate("user", "geo_restricted", &context);
        let region_specific = engine.evaluate("user", "region_specific", &context);

        println!(
            "  {}: Geo Restricted: {}, Region Specific: {}",
            description,
            format_result(!geo_restricted), // Inverted because it's a DENY policy
            format_result(region_specific)
        );
    }

    println!();
    Ok(())
}

fn format_result(allowed: bool) -> String {
    if allowed {
        "✅ ALLOW".to_string()
    } else {
        "❌ DENY".to_string()
    }
}
