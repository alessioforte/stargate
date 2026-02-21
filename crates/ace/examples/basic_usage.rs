use ace::{PolicyEngine, Value, context_with};
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a new policy engine
    let mut engine = PolicyEngine::new();

    // Define some policies as comments (like they would appear in source code)
    let policy_content = r#"
        ALLOW user FOR "dashboard" WHEN user.role == "admin" OR user.role == "manager";
        ALLOW user FOR "reports" WHEN user.role == "admin" OR user.role == "analyst";
        DENY user FOR "admin_panel" WHEN user.department == "guest";
        ALLOW user FOR "profile" WHEN user.active == true;

        ALLOW api_key FOR "read_data" WHEN api_key.valid == true AND api_key.scope == "read";
        ALLOW api_key FOR "write_data" WHEN api_key.valid == true AND api_key.scope == "write";
        DENY api_key FOR "delete_data" WHEN api_key.scope != "admin";
    "#;

    // Parse the policies
    engine.parse_file(policy_content)?;

    println!("Loaded {} policies", engine.get_policies().len());

    // Test user access scenarios
    println!("\n--- User Access Tests ---");

    // Admin user
    let admin_context = context_with(vec![
        ("user.role", Value::String("admin".to_string())),
        ("user.active", Value::Boolean(true)),
        ("user.department", Value::String("engineering".to_string())),
    ]);

    test_access(
        &engine,
        "user",
        "dashboard",
        &admin_context,
        "Admin user accessing dashboard",
    );
    test_access(
        &engine,
        "user",
        "reports",
        &admin_context,
        "Admin user accessing reports",
    );
    test_access(
        &engine,
        "user",
        "profile",
        &admin_context,
        "Admin user accessing profile",
    );

    // Manager user
    let manager_context = context_with(vec![
        ("user.role", Value::String("manager".to_string())),
        ("user.active", Value::Boolean(true)),
        ("user.department", Value::String("sales".to_string())),
    ]);

    test_access(
        &engine,
        "user",
        "dashboard",
        &manager_context,
        "Manager user accessing dashboard",
    );
    test_access(
        &engine,
        "user",
        "reports",
        &manager_context,
        "Manager user accessing reports",
    );

    // Guest user
    let guest_context = context_with(vec![
        ("user.role", Value::String("guest".to_string())),
        ("user.active", Value::Boolean(true)),
        ("user.department", Value::String("guest".to_string())),
    ]);

    test_access(
        &engine,
        "user",
        "admin_panel",
        &guest_context,
        "Guest user accessing admin panel",
    );
    test_access(
        &engine,
        "user",
        "profile",
        &guest_context,
        "Guest user accessing profile",
    );

    // Inactive user
    let inactive_context = context_with(vec![
        ("user.role", Value::String("admin".to_string())),
        ("user.active", Value::Boolean(false)),
        ("user.department", Value::String("engineering".to_string())),
    ]);

    test_access(
        &engine,
        "user",
        "profile",
        &inactive_context,
        "Inactive user accessing profile",
    );

    // Test API key scenarios
    println!("\n--- API Key Access Tests ---");

    // Valid read API key
    let read_api_context = context_with(vec![
        ("api_key.valid", Value::Boolean(true)),
        ("api_key.scope", Value::String("read".to_string())),
    ]);

    test_access(
        &engine,
        "api_key",
        "read_data",
        &read_api_context,
        "Read API key accessing data",
    );
    test_access(
        &engine,
        "api_key",
        "write_data",
        &read_api_context,
        "Read API key trying to write data",
    );

    // Valid write API key
    let write_api_context = context_with(vec![
        ("api_key.valid", Value::Boolean(true)),
        ("api_key.scope", Value::String("write".to_string())),
    ]);

    test_access(
        &engine,
        "api_key",
        "read_data",
        &write_api_context,
        "Write API key accessing data",
    );
    test_access(
        &engine,
        "api_key",
        "write_data",
        &write_api_context,
        "Write API key writing data",
    );
    test_access(
        &engine,
        "api_key",
        "delete_data",
        &write_api_context,
        "Write API key trying to delete data",
    );

    // Admin API key
    let admin_api_context = context_with(vec![
        ("api_key.valid", Value::Boolean(true)),
        ("api_key.scope", Value::String("admin".to_string())),
    ]);

    test_access(
        &engine,
        "api_key",
        "delete_data",
        &admin_api_context,
        "Admin API key deleting data",
    );

    // Invalid API key
    let invalid_api_context = context_with(vec![
        ("api_key.valid", Value::Boolean(false)),
        ("api_key.scope", Value::String("read".to_string())),
    ]);

    test_access(
        &engine,
        "api_key",
        "read_data",
        &invalid_api_context,
        "Invalid API key accessing data",
    );

    // Demonstrate detailed evaluation
    println!("\n--- Detailed Evaluation ---");

    let result = engine.evaluate_with_details("user", "dashboard", &admin_context);
    println!("Evaluation details for admin accessing dashboard:");
    println!(
        "  Decision: {}",
        if result.is_allowed() { "ALLOW" } else { "DENY" }
    );
    println!("  Matched policies: {:?}", result.matched_policies);
    println!("  Applied policies: {:?}", result.applied_policies);
    println!("  Allow count: {}", result.allow_count);
    println!("  Deny count: {}", result.deny_count);

    // Show how to get matching policies
    println!("\n--- Policy Inspection ---");
    let matching = engine.get_matching_policies("user", "dashboard");
    println!("Policies matching 'user' subject and 'dashboard' resource:");
    for (i, policy) in matching.iter().enumerate() {
        println!("  {}: {}", i + 1, policy);
    }

    // Export policies
    println!("\n--- Policy Export ---");
    let exported = engine.export_policies();
    println!("Exported policies:");
    for policy in exported {
        println!("  {}", policy);
    }

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
