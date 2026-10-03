# ACE (Access Control Engine) for Stargate

A flexible policy-based access control engine that parses textual policy definitions and evaluates them against runtime context. Supports rich value types (strings, numbers, booleans, dates, times, datetimes with timezones, arrays), substring/membership checks, and JSON-context interop.

## Features

- **Plain-text policy syntax** — load from files, strings, or build programmatically
- **Boolean conditions** — `AND`, `OR`, `NOT`, with parentheses
- **Comparison + membership operators** — `==`, `!=`, `>`, `<`, `>=`, `<=`, `CONTAINS`
- **Rich value types** — strings, integers, floats, booleans, dates, times, datetimes (with timezone), arrays
- **Timezone-aware time/datetime comparisons** — compare values across offsets, including `Z` (UTC) shorthand
- **Resource actions** — fine-grained `READ`/`WRITE`/`DELETE`/`CREATE`/`UPDATE`/`EXECUTE`/`ADMIN`/`*` qualifiers
- **Detailed evaluation results** — see which policies matched, which applied, and allow/deny counts
- **Indexed lookup** — `(subject, resource)` lookup is O(1); only candidate policies are evaluated
- **`serde_json::Value` interop** — drop JSON values into the context with automatic datetime detection
- **`ContextBuilder`** — fluent helper for common context fields
- **Multi-source loading** — load from many policy strings at once
- **Comment support** — lines starting with `#` are ignored in policy files

## Policy Syntax

```text
ALLOW|DENY <subject> FOR "<resource>[:ACTION]" [WHEN <condition>];
```

- `ALLOW`/`DENY` — decision the policy expresses
- `subject` — entity requesting access (e.g. `user`, `api_key`, `service`)
- `resource` — quoted resource identifier; optional `:ACTION` suffix scopes the policy to one action
- `condition` — optional boolean expression

Lines starting with `#` and blank lines are skipped.

### Operators

**Comparison:**

| Op   | Meaning                  |
|------|--------------------------|
| `==` | Equal                    |
| `!=` | Not equal                |
| `>`  | Greater than             |
| `<`  | Less than                |
| `>=` | Greater than or equal    |
| `<=` | Less than or equal       |

**Membership / substring:**

| Op         | Meaning                                                  |
|------------|----------------------------------------------------------|
| `CONTAINS` | Array contains element, or string contains substring     |

**Logical** (precedence highest → lowest): `NOT`, `AND`, `OR`. Parentheses supported for grouping.

### Value Types

| Type      | Examples                                                       |
|-----------|----------------------------------------------------------------|
| String    | `"admin"`, `'read'` (single or double quotes)                  |
| Boolean   | `true`, `false`                                                |
| Integer   | `42`, `-10`                                                    |
| Float     | `3.14`, `-2.7`                                                 |
| Date      | `2026-01-01` (unquoted ISO-8601, or quoted)                    |
| Time      | `"09:00"`, `"17:30:00"` (quoted `HH:MM[:SS]`)                  |
| DateTime  | `"2026-01-01T09:00:00+02:00"`, `"09:00+02:00"`, `"17:00Z"`     |
| Array     | Built only via context (e.g. `Value::Array(vec![...])`)        |

Quoted strings are auto-parsed as `DateTime` → `Date` → `Time` → `String` (first match wins). String context values compare correctly against date/time/datetime literals via on-the-fly coercion.

### Resource Actions

A policy targets a specific action via `"resource:ACTION"`:

`READ`, `WRITE`, `DELETE`, `CREATE`, `UPDATE`, `EXECUTE`, `ADMIN`, `*` (or `ANY`).

Matching rules at evaluation time:
- Policy **without** an action → matches any request (with or without an action).
- Policy with `ANY`/`*` → matches any action.
- Policy with a specific action → matches only that action; does **not** match an action-less request.

## Quick Start

```rust
use ace::{PolicyEngine, context_with, Value};

let mut engine = PolicyEngine::new();

let policies = r#"
    # Comments and blank lines are ignored
    ALLOW user FOR "dashboard" WHEN user.role == "admin";
    ALLOW user FOR "reports"   WHEN user.role == "admin" OR user.role == "analyst";
    DENY  user FOR "dashboard" WHEN user.suspended == true;
"#;
engine.parse_file(policies)?;

let ctx = context_with(vec![
    ("user.role",      Value::String("admin".into())),
    ("user.suspended", Value::Boolean(false)),
]);

assert!(engine.evaluate("user", "dashboard", &ctx));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Conditions

```text
ALLOW user FOR "sensitive_data" WHEN user.role == "admin" AND user.mfa_enabled == true;
ALLOW user FOR "secure_area"    WHEN NOT user.suspended == true;
DENY  user FOR "restricted"     WHEN NOT (user.clearance == "secret" AND location == "office");

ALLOW user FOR "adult_content"     WHEN user.age >= 18;
ALLOW user FOR "premium_features"  WHEN user.score > 95.0 AND user.subscription == "premium";

ALLOW user FOR "database:READ"   WHEN user.role == "analyst";
ALLOW user FOR "database:WRITE"  WHEN user.role == "admin" AND user.mfa_enabled == true;
DENY  user FOR "database:DELETE" WHEN user.probation == true;
```

### CONTAINS — arrays and substrings

```text
ALLOW user FOR "admin_panel"      WHEN user.roles  CONTAINS "admin";
DENY  user FOR "feature1"         WHEN user.tags   CONTAINS "banned";
ALLOW user FOR "internal"         WHEN user.email  CONTAINS "@company.com";
ALLOW user FOR "verified_feature" WHEN NOT user.tags CONTAINS "unverified";
```

```rust
use ace::{context_with, Value};

let ctx = context_with(vec![(
    "user.roles",
    Value::Array(vec![
        Value::String("editor".into()),
        Value::String("admin".into()),
    ]),
)]);
```

### Dates, times, and timezones

```text
# Date literal (unquoted ISO date)
ALLOW user FOR "feature1" WHEN time.date >= 2026-01-01;

# Time-of-day with timezone offset
DENY  user FOR "after_hours" WHEN env.time < "09:00+02:00" OR env.time > "17:00+02:00";

# Full datetime
ALLOW user FOR "campaign" WHEN time.datetime >= "2026-01-01T00:00:00Z";
```

Timezone-aware comparisons normalize across offsets, so `"09:00+02:00"` compares equal to `"07:00+00:00"`. The `Z` suffix is accepted as `+00:00`.

String context values are coerced when compared against typed literals — `("time.date", Value::String("2026-02-15".into()))` compares correctly against `Value::Date(...)`.

## API Cheatsheet

```rust
use ace::{PolicyEngine, ResourceAction, Value, context, context_with};

let mut engine = PolicyEngine::new();

// Loading
engine.parse_file(text)?;                          // skips blanks/comments/invalid lines
engine.load_from_sources(vec![src1, src2])?;       // multiple sources
let policy = engine.parse_policy(line)?;           // parse a single line (no insert)
engine.add_policy(policy);                          // insert programmatically
engine.clear_policies();                            // wipe all
let dump: Vec<String> = engine.export_policies();   // canonical text form

// Evaluation
engine.evaluate("user", "dashboard", &ctx);
engine.evaluate_with_action("user", "database", &ResourceAction::Read, &ctx);
let det = engine.evaluate_with_details("user", "dashboard", &ctx);
let det = engine.evaluate_with_details_and_action("user", "database", &ResourceAction::Read, &ctx);

// Inspection
let all     = engine.get_policies();
let matched = engine.get_matching_policies("user", "dashboard");
let matched = engine.get_matching_policies_with_action("user", "database", &ResourceAction::Read);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Detailed Evaluation Results

```rust
let result = engine.evaluate_with_details("user", "resource", &ctx);

result.is_allowed();           // bool — final decision
result.is_denied();
result.has_explicit_decision();// at least one policy applied
result.matched_policies;       // indices of subject/resource/action matches
result.applied_policies;       // subset whose condition evaluated true
result.allow_count;
result.deny_count;
```

### Programmatic Policy Construction

```rust
use ace::{Policy, PolicyAction, Condition, Expression, Operator, Value};

let policy = Policy::new(PolicyAction::Allow, "user".into(), "admin_panel".into())
    .with_condition(Condition::And(
        Box::new(Condition::Expression(Expression {
            left: "user.role".into(),
            operator: Operator::Equal,
            right: Value::String("admin".into()),
        })),
        Box::new(Condition::Expression(Expression {
            left: "user.mfa_enabled".into(),
            operator: Operator::Equal,
            right: Value::Boolean(true),
        })),
    ));
```

### ContextBuilder

```rust
use ace::{ContextBuilder, Value};

let ctx = ContextBuilder::new()
    .user_role("admin")
    .user_department("security")
    .location("office")
    .time_of_day("business_hours")
    .day_of_week("tuesday")
    .date("2026-01-15")                  // -> Value::Date or DateTime if offset present
    .time("09:30+02:00")                 // -> Value::DateTime; "09:30" -> Value::Time
    .datetime("2026-01-15T09:30:00Z")    // -> Value::DateTime
    .ip_address("203.0.113.4")
    .device_type("desktop")
    .security_level(4)
    .add("user.mfa_enabled", Value::Boolean(true))
    .add_array("user.permissions", vec![
        Value::String("read".into()),
        Value::String("write".into()),
    ])
    .build();
```

### JSON Interop

`Value` implements `From<serde_json::Value>` and `From<&serde_json::Value>`. The borrowed conversion additionally tries to parse string scalars as `DateTime`, `Date`, or `Time` before falling back to `String`. Useful when feeding HTTP/JSON request payloads into the context:

```rust
use ace::Value;
use serde_json::json;

let body = json!({
    "user": { "role": "admin", "joined_at": "2026-01-15T09:30:00Z" },
    "tags": ["beta", "vip"]
});

let role: Value   = (&body["user"]["role"]).into();        // Value::String
let joined: Value = (&body["user"]["joined_at"]).into();   // Value::DateTime
let tags: Value   = (&body["tags"]).into();                // Value::Array(...)
```

## Policy Resolution

1. **Deny wins**: any matching `DENY` whose condition evaluates true → access denied.
2. **Explicit allow required**: at least one matching `ALLOW` whose condition is true must exist.
3. **Default deny**: no matches, or no condition true → denied.
4. **Order independent**: deny-wins makes order irrelevant.
5. **Missing context key**: any expression referencing an undefined key evaluates to `false`.

## Performance

- **Parsing**: O(n) over input lines.
- **Lookup**: `(subject, resource)` candidates indexed in a `HashMap` → O(1) lookup.
- **Evaluation**: O(c) over candidate policies (typically a handful), not the full policy set.
- **Memory**: AST stored once; lookup index stores `usize` offsets into the policy vector.

## Use Cases

### Web App Authorization

```text
ALLOW user FOR "dashboard:READ"   WHEN user.role == "admin" OR user.role == "manager";
ALLOW user FOR "dashboard:WRITE"  WHEN user.role == "admin";
ALLOW user FOR "beta_features"    WHEN user.beta_tester == true AND NOT user.banned == true;
ALLOW user FOR "premium_features" WHEN user.score >= 95.5 AND user.subscription == "premium";
```

### API Access Control

```text
ALLOW api_key FOR "users:READ"   WHEN api_key.valid == true AND api_key.scope == "read";
ALLOW api_key FOR "users:WRITE"  WHEN api_key.valid == true AND api_key.scope == "write";
ALLOW api_key FOR "users:DELETE" WHEN api_key.valid == true AND api_key.scope == "admin";
DENY  api_key FOR "high_volume"  WHEN api_key.requests_per_minute > 100;
ALLOW service FOR "internal_api" WHEN service.verified == true AND security.level >= 3;
```

### Compliance / Security

```text
ALLOW user FOR "financial_data:READ"  WHEN user.sox_certified == true AND NOT user.foreign_national == true;
ALLOW user FOR "patient_data:READ"    WHEN user.hipaa_certified == true AND user.department == "healthcare";
DENY  user FOR "patient_data:*"       WHEN user.background_check != "completed";
DENY  user FOR "restricted_content"   WHEN request.country == "blocked" OR request.ip_suspicious == true;
ALLOW user FOR "after_hours_access"   WHEN env.time >= "20:00+00:00" AND (user.on_call == true OR user.role == "admin");
ALLOW user FOR "classified:READ"      WHEN security.level >= 3 AND user.mfa_enabled == true AND NOT user.suspended == true;
```

### Membership / Tag Checks

```text
ALLOW user FOR "internal"         WHEN user.email CONTAINS "@company.com";
ALLOW user FOR "admin_panel"      WHEN user.roles CONTAINS "admin";
ALLOW user FOR "verified_feature" WHEN NOT user.tags CONTAINS "unverified";
```

## Examples

```bash
cargo run --example basic_usage         # core API walkthrough
cargo run --example advanced_usage      # complex conditions
cargo run --example extended_features   # operators, actions, contexts
cargo run --example practical_usage     # end-to-end scenarios
```

A sample policy file is at `examples/policies.txt`.

## Testing

```bash
cargo test
```

Coverage includes parser variants, evaluator semantics, timezone-aware comparisons, array/string `CONTAINS`, indexed lookup, JSON coercion, and detailed-result aggregation.

## Error Handling

```rust
match engine.parse_file(input) {
    Ok(_)  => println!("loaded"),
    Err(e) => eprintln!("parse error: {e}"),
}
```

`ParseError` variants:

- `InvalidSyntax` — malformed structure
- `UnexpectedToken` — bad keyword/operator
- `MissingToken` — expected `FOR`/`WHEN`
- `InvalidValue` — unparseable literal

Note: `parse_file` skips lines that fail to parse rather than aborting the load — use `parse_policy(line)` directly if you need strict parsing of individual lines.

## Common Context Variables

These are conventions, not requirements — any key works.

**User:** `user.role`, `user.department`, `user.age`, `user.active`, `user.suspended`, `user.mfa_enabled`, `user.score`, `user.clearance_level`, `user.experience_months`, `user.roles` (array), `user.tags` (array), `user.email`

**Time:** `time.of_day`, `time.day_of_week`, `time.date`, `time.time`, `time.datetime`, `env.time`

**Location / Request:** `location`, `request.country`, `request.ip`, `request.ip_suspicious`

**Device:** `device.type`, `device.secure`, `device.biometric_enabled`, `device.registered`

**Security / API:** `security.level`, `api_key.valid`, `api_key.scope`, `api_key.rate_limited`

## Integration with Stargate

The ACE engine powers the `access_control` policy kind in the v1 gateway router pipeline (`docs/config-v1.md`) and is also used for admin/IAM access decisions. Policy files are hot-reloadable via the gateway's config watcher.

## License

Same terms as the parent Stargate project.
