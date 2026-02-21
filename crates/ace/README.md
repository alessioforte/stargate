# ACE (Access Control Engine) for Stargate

A powerful, flexible policy-based access control engine that parses policy definitions and evaluates them against runtime context.

## Features

- **Simple Policy Syntax**: Define policies in plain text files or inline strings
- **Flexible Conditions**: Support for complex boolean expressions with AND/OR logic
- **Multiple Value Types**: String, boolean, and numeric values
- **Detailed Evaluation**: Get detailed results about which policies matched and applied
- **High Performance**: Efficient parsing and evaluation suitable for production use
- **Extensible**: Easy to extend with custom operators and value types

## Policy Syntax

Policies are defined using the following syntax:

```text
ALLOW|DENY subject FOR "resource" [WHEN condition];
```

Where:
- `ALLOW|DENY` - The action to take
- `subject` - The entity requesting access (e.g., "user", "api_key", "service")
- `resource` - The resource being accessed (quoted string)
- `condition` - Optional boolean expression using AND/OR logic

### Supported Operators

**Comparison Operators:**
- `==` - Equals
- `!=` - Not equals
- `>` - Greater than
- `<` - Less than
- `>=` - Greater than or equal
- `<=` - Less than or equal

**Logical Operators:**
- `AND` - Logical and (higher precedence)
- `OR` - Logical or (lower precedence)
- `NOT` - Logical not (highest precedence)

### Supported Value Types

- **Strings**: `"admin"`, `"read"`, `"write"`
- **Booleans**: `true`, `false`
- **Integers**: `1`, `42`, `-10`
- **Floats**: `3.14`, `95.5`, `-2.7`

### Resource Actions

Resources can specify specific actions using the syntax `"resource:ACTION"`:

- `READ` - Read access
- `WRITE` - Write access
- `DELETE` - Delete access
- `CREATE` - Create access
- `UPDATE` - Update access
- `EXECUTE` - Execute access
- `ADMIN` - Administrative access
- `*` or `ANY` - Any action

## Examples

### Basic Usage

```rust
use ace::{PolicyEngine, context_with, Value};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = PolicyEngine::new();

    let policies = r#"
        ALLOW user FOR "dashboard" WHEN user.role == "admin";
        ALLOW user FOR "reports" WHEN user.role == "admin" OR user.role == "analyst";
        DENY user FOR "admin_panel" WHEN user.suspended == true;
    "#;

    // Parse the policies
    engine.parse_file(policies)?;

    // Create context for evaluation
    let context = context_with(vec![
        ("user.role", Value::String("admin".to_string())),
        ("user.suspended", Value::Boolean(false)),
    ]);

    // Evaluate access
    let allowed = engine.evaluate("user", "dashboard", &context);
    println!("Access allowed: {}", allowed); // true

    Ok(())
}
```

### Complex Conditions

```text
ALLOW user FOR "sensitive_data" WHEN user.role == "admin" AND user.mfa_enabled == true;

ALLOW user FOR "secure_area" WHEN NOT user.suspended == true;
DENY user FOR "restricted" WHEN NOT (user.clearance == "secret" AND user.location == "office");

ALLOW user FOR "adult_content" WHEN user.age >= 18;
ALLOW user FOR "premium_features" WHEN user.score > 95.0 AND user.subscription == "premium";

ALLOW user FOR "database:READ" WHEN user.role == "analyst";
ALLOW user FOR "database:WRITE" WHEN user.role == "admin" AND user.mfa_enabled == true;
DENY user FOR "database:DELETE" WHEN user.probation == true;

ALLOW user FOR "after_hours_access" WHEN time.of_day == "night" AND user.on_call == true;
DENY user FOR "office_resources" WHEN location != "office" AND NOT user.vpn_connected == true;
```

### Working with API Keys

```rust
use ace::{PolicyEngine, context_with, Value};

let mut engine = PolicyEngine::new();

let api_policies = r#"
    ALLOW api_key FOR "public_api" WHEN api_key.valid == true;
    ALLOW api_key FOR "admin_api" WHEN api_key.valid == true AND api_key.scope == "admin";
    DENY api_key FOR "rate_limited" WHEN api_key.rate_limited == true;
"#;

engine.parse_file(api_policies)?;

let api_context = context_with(vec![
    ("api_key.valid", Value::Boolean(true)),
    ("api_key.scope", Value::String("admin".to_string())),
    ("api_key.rate_limited", Value::Boolean(false)),
]);

let allowed = engine.evaluate("api_key", "admin_api", &api_context);
println!("API access allowed: {}", allowed); // true

// Evaluate with specific resource action
let write_allowed = engine.evaluate_with_action(
    "user",
    "database",
    &ResourceAction::Write,
    &context
);
```

## Advanced Features

### Detailed Evaluation Results

Get detailed information about policy evaluation:

```rust
let result = engine.evaluate_with_details("user", "resource", &context);

println!("Decision: {}", if result.is_allowed() { "ALLOW" } else { "DENY" });
println!("Matched policies: {:?}", result.matched_policies);
println!("Applied policies: {:?}", result.applied_policies);
println!("Allow count: {}", result.allow_count);
println!("Deny count: {}", result.deny_count);
```

### Policy Inspection

Find policies that match specific criteria:

```rust
let matching_policies = engine.get_matching_policies("user", "dashboard");
for policy in matching_policies {
    println!("Policy: {}", policy);
}
```

### Programmatic Policy Creation

Create policies programmatically using the builder pattern:

```rust
use ace::{Policy, PolicyAction, Condition, Expression, Operator, Value};

let policy = Policy::new(
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

engine.add_policy(policy);
```

### Rich Context Building

Use the `ContextBuilder` for complex context scenarios:

```rust
use ace::{ContextBuilder, Value};

let context = ContextBuilder::new()
    .user_role("admin")
    .user_department("security")
    .location("office")
    .time_of_day("business_hours")
    .device_type("desktop")
    .security_level(4)
    .add("user.mfa_enabled", Value::Boolean(true))
    .add("user.clearance", Value::String("top_secret".to_string()))
    .build();

let allowed = engine.evaluate("user", "classified_data", &context);
```

### Multiple Policy Sources

Load policies from multiple sources:

```rust
let base_policies = "ALLOW user FOR \"login\" WHEN user.active == true;";
let admin_policies = "ALLOW user FOR \"admin\" WHEN user.role == \"admin\";";

engine.load_from_sources(vec![base_policies, admin_policies])?;
```

## Policy Resolution

The engine follows these rules for policy resolution:

1. **Explicit Deny Wins**: If any DENY policy matches and its condition is true, access is denied
2. **Explicit Allow Required**: Access is only granted if at least one ALLOW policy matches and its condition is true
3. **Default Deny**: If no policies match or all matching policies have false conditions, access is denied
4. **Order Independence**: Policy order doesn't matter due to the explicit deny-wins rule

## Performance Characteristics

- **Parsing**: O(n) where n is the number of policy lines
- **Evaluation**: O(p) where p is the number of policies for the subject/resource pair
- **Memory**: Policies are stored in an efficient AST structure
- **Scalability**: Tested with thousands of policies with sub-millisecond evaluation times

## Use Cases

### Web Application Authorization

```text
ALLOW user FOR "dashboard:READ" WHEN user.role == "admin" OR user.role == "manager";
ALLOW user FOR "dashboard:WRITE" WHEN user.role == "admin";

ALLOW user FOR "adult_content" WHEN user.age >= 18;
DENY user FOR "teen_content" WHEN user.age > 17;

ALLOW user FOR "beta_features" WHEN user.beta_tester == true AND NOT user.banned == true;

ALLOW user FOR "premium_features" WHEN user.score >= 95.5 AND user.subscription == "premium";
```

### API Access Control

```text
ALLOW api_key FOR "users:READ" WHEN api_key.valid == true AND api_key.scope == "read";
ALLOW api_key FOR "users:WRITE" WHEN api_key.valid == true AND api_key.scope == "write";
ALLOW api_key FOR "users:DELETE" WHEN api_key.valid == true AND api_key.scope == "admin";

DENY api_key FOR "high_volume_api" WHEN api_key.requests_per_minute > 100;

ALLOW service FOR "internal_api" WHEN service.verified == true AND security.level >= 3;
```

### Compliance and Security

```text
ALLOW user FOR "financial_data:READ" WHEN user.sox_certified == true AND NOT user.foreign_national == true;
ALLOW user FOR "financial_data:WRITE" WHEN user.sox_certified == true AND user.clearance_level >= 3;

ALLOW user FOR "patient_data:READ" WHEN user.hipaa_certified == true AND user.department == "healthcare";
DENY user FOR "patient_data:*" WHEN user.background_check != "completed";

DENY user FOR "restricted_content" WHEN request.country == "blocked" OR request.ip_suspicious == true;
ALLOW user FOR "after_hours_access" WHEN time.of_day == "night" AND (user.on_call == true OR user.role == "admin");

ALLOW user FOR "classified:READ" WHEN security.level >= 3 AND user.mfa_enabled == true AND NOT user.suspended == true;
```

## Running Examples

The crate includes comprehensive examples:

```bash
# Basic usage example
cargo run --example basic_usage

# Advanced scenarios with complex conditions
cargo run --example advanced_usage

# Extended features: operators, actions, contexts
cargo run --example extended_features
```

## Testing

Run the test suite:

```bash
cargo test
```

The test suite includes:
- Parser tests for all syntax variations
- Evaluator tests for complex conditions
- Integration tests for real-world scenarios
- Performance benchmarks

## Error Handling

The engine provides detailed error information for invalid policies:

```rust
match engine.parse_file(invalid_policies) {
    Ok(_) => println!("Policies loaded successfully"),
    Err(e) => println!("Parse error: {}", e),
}
```

Common parse errors include:
- `InvalidSyntax`: Malformed policy structure
- `UnexpectedToken`: Invalid keywords or operators
- `MissingToken`: Missing required elements like "FOR" or "WHEN"
- `InvalidValue`: Unsupported value types or formats

## Contributing

We welcome contributions! Please see our contribution guidelines for:
- Code style and conventions
- Testing requirements
- Documentation standards
- Performance considerations

## License

This project is licensed under the same terms as the parent Stargate project.

## Integration with Stargate

The ACE engine is designed to integrate seamlessly with the Stargate ecosystem:

- **Database Access Control**: Control access to tables, columns, and operations
- **API Gateway**: Secure REST and GraphQL endpoints
- **Service Mesh**: Inter-service communication policies
- **Admin Interface**: Role-based administrative access

For more information about Stargate, see the main project documentation.

## Extended Syntax Examples

### Complete Policy Examples

```text
ALLOW user FOR "alcohol_purchase" WHEN user.age >= 21 AND user.id_verified == true;

ALLOW user FOR "classified:READ" WHEN user.clearance == "secret" AND security.level >= 3 AND NOT user.foreign_national == true;

ALLOW user FOR "mobile_banking:WRITE" WHEN device.type == "mobile" AND device.biometric_enabled == true AND NOT (time.of_day == "night" AND location != "home");

ALLOW user FOR "database:READ" WHEN user.role == "analyst" OR user.role == "admin";
ALLOW user FOR "database:WRITE" WHEN user.role == "admin" AND NOT user.probation == true;
DENY user FOR "database:DELETE" WHEN user.experience_months < 12 OR user.approval_required == true;

ALLOW user FOR "premium_features" WHEN user.score >= 95.5 AND (user.subscription == "premium" OR user.subscription == "enterprise");

DENY user FOR "eu_data:*" WHEN request.country != "EU" AND NOT user.gdpr_authorized == true;
ALLOW user FOR "us_only:*" WHEN request.country == "US" AND user.citizenship == "US";
```

### Context Variables

Common context variables used in policies:

**User Context:**
- `user.role`, `user.department`, `user.age`
- `user.active`, `user.suspended`, `user.mfa_enabled`
- `user.score`, `user.clearance_level`, `user.experience_months`

**Time Context:**
- `time.of_day` ("morning", "day", "evening", "night")
- `time.day_of_week` ("monday", "tuesday", etc.)
- `time.business_hours` (boolean)

**Location Context:**
- `location` ("office", "home", "remote")
- `request.country`, `request.ip`, `request.ip_suspicious`

**Device Context:**
- `device.type` ("desktop", "mobile", "tablet")
- `device.secure`, `device.biometric_enabled`, `device.registered`

**Security Context:**
- `security.level` (numeric: 1-5)
- `api_key.valid`, `api_key.scope`, `api_key.rate_limited`
