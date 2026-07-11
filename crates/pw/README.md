# pw — Passwords for Stargate

Password primitives for the Stargate IAM server: Argon2id hashing with
tunable costs and an optional pepper, a serde-configurable password policy
engine with typed violations, a class-guaranteeing password generator, and
API-key generation/hashing helpers.

The crate is deliberately application-agnostic: no Axum, no database, no
env-var parsing. Stargate wires it up in `src/etc/pw.rs` (hashing config) and
`src/act/password_policy.rs` (policy resolution and enforcement). For the
operator-facing view — env vars, per-organization overrides, login flows —
see [`docs/password-policies-guide.md`](../../docs/password-policies-guide.md).

## Features

- **Argon2id hashing** — PHC-format hashes, salted per password
- **Tunable costs** — memory / iterations / parallelism set once at boot via `Hash::configure`
- **Optional pepper** — applied as the Argon2 `secret` (keyed hashing), with a migration-safe unpeppered fallback
- **`needs_rehash` detection** — spot hashes created with outdated parameters or without the pepper, so the app can transparently re-encode on login
- **Policy engine** — parameterized rule set (`PasswordPolicySet`), serde-deserializable from JSON/YAML, snake_case canonical with camelCase aliases, unknown fields rejected
- **Typed violations** — `PolicyViolation` with stable machine codes and human messages; `validate_all` returns every violation at once
- **Context-aware rules** — reject passwords containing the username or email
- **Banned-password list** — built-in common passwords, extensible with operator-supplied lists
- **Stateful policy fields** — `max_age_days` (expiry) and `history` (no reuse) carried in the same config model, enforced by the application
- **Password generator** — builder API that guarantees at least one character per enabled class
- **API keys** — `sk_live_…`-style opaque keys (256-bit) and SHA-256 hashing for storage

## Modules

| Module | Contents |
|--------|----------|
| `hash` | `Hash`, `HashConfig`, `Verification` — Argon2 encode/verify/rehash detection |
| `policies` | `PasswordPolicySet`, `PolicyContext`, `PolicyViolation`, `MAX_PASSWORD_BYTES`, `MAX_HISTORY_DEPTH` |
| `banned` | `BannedPasswords` — built-in + extensible common-password list |
| `generator` | `Generator` — random password/secret builder |
| `keys` | `generate_api_key`, `generate_admin_key`, `hash_api_key` |

## Hashing

```rust
use pw::{Hash, HashConfig};

// Once at boot (optional — defaults are Argon2id m=19456 KiB, t=2, p=1, no pepper):
let config = HashConfig::new(19_456, 2, 1, Some("my-pepper".to_string()))?;
Hash::configure(config)?;

// Hash and verify:
let hash = Hash::encode("hunter2!aA")?;          // "$argon2id$v=19$m=19456,t=2,p=1$..."
assert!(Hash::verify("hunter2!aA", &hash).is_ok());
```

### Rehash-on-login support

`verify_for_login` verifies and reports whether the stored hash should be
re-encoded while the plaintext is at hand:

```rust
let verification = pw::Hash::verify_for_login("hunter2!aA", &stored_hash)?;
if verification.needs_rehash {
    let upgraded = pw::Hash::encode("hunter2!aA")?;
    // persist `upgraded` (Stargate: `rehash_credential`, which preserves
    // updated_at and skips password history)
}
```

`needs_rehash` is `true` when the stored hash:

- uses different cost parameters than the current configuration,
- is not Argon2id, or is unparseable, or
- was created **without** the now-configured pepper.

The pepper case works through a migration fallback: when a pepper is
configured and the peppered check fails, verification retries with
`Argon2::default()` (unpeppered). A match there succeeds with
`needs_rehash: true`, so pre-pepper hashes keep verifying and upgrade on the
next login. Note the fallback covers *introducing* a pepper, not *rotating*
one — hashes made with an old pepper will not verify under a new one.

`Hash::matches_any(password, hashes)` verifies against a list (used for
password-history checks). Each comparison is a full Argon2 run — treat it as
blocking CPU work and call it off the async runtime.

## Password policies

`PasswordPolicySet` is plain serde data, so the same shape works from env
plumbing, YAML config, or a JSON attrs blob:

```rust
use pw::{PasswordPolicySet, PolicyContext};

let policy: PasswordPolicySet = serde_json::from_str(
    r#"{ "min_length": 12, "specialChars": false, "max_repeats": 3 }"#,
)?;
policy.check().expect("policy is self-consistent");

let ctx = PolicyContext {
    username: Some("alessio"),
    email: Some("alessio@example.com"),
    banned: None, // None = built-in banned list
};

// First violation, or all of them:
policy.validate("candidate-password", &ctx)?;
let violations = policy.validate_all("candidate-password", &ctx);
for violation in &violations {
    println!("{}: {}", violation.code(), violation); // "too_short: Password must be at least..."
}
```

Deserialization rules:

- snake_case canonical, camelCase aliases accepted (`min_length` / `minLength`)
- unknown fields are **rejected** (typos cannot silently weaken a policy)
- missing fields take the `PasswordPolicySet::standard()` values, so partial
  configs strengthen rather than weaken; disable rules explicitly with
  `false` / `null`

### Rules

| Field | Default | Violation code | Checks |
|-------|---------|----------------|--------|
| `min_length` | `8` | `too_short` | Minimum length in **characters** (not bytes) |
| `max_length` | `64` | `too_long` | Maximum length in characters |
| `lowercase` | `true` | `missing_lowercase` | At least one lowercase letter |
| `uppercase` | `true` | `missing_uppercase` | At least one uppercase letter |
| `digits` | `true` | `missing_digit` | At least one digit |
| `special_chars` | `true` | `missing_special_char` | At least one non-alphanumeric character |
| `not_username` | `true` | `contains_username` | Password must not contain the username (case-insensitive; needles under 3 chars ignored) |
| `not_email` | `true` | `contains_email` | Must not contain the email or its local part |
| `banned_passwords` | `true` | `banned_password` | Not on the banned/common list (case-insensitive) |
| `regex_pattern` | off | `pattern_mismatch` | Must match the regex (`is_match` semantics) |
| `max_repeats` | off | `repeated_characters` | No run of the same character longer than N |
| `max_age_days` | off | — (stateful) | Password expiry, enforced by the app at login |
| `history` | off | `recently_used` (stateful) | No reuse of the last N passwords (≤ `MAX_HISTORY_DEPTH` = 24), enforced by the app on change |

Two hard limits apply regardless of configuration:

- `MAX_PASSWORD_BYTES` (512): checked before any rule and before hashing.
- Stateful fields (`max_age_days`, `history`) are **never** evaluated by
  `validate_all` — they need a clock and stored hashes, which the application
  owns. The crate contributes the config fields, the `recently_used`
  violation, and `Hash::matches_any`.

`check()` validates the policy itself (min ≤ max, regex compiles,
`max_repeats`/`max_age_days` ≥ 1, `history` within 1..=24). Call it at
config-load time; an invalid `regex_pattern` at validate time is skipped
rather than locking everyone out.

### Banned passwords

```rust
use pw::BannedPasswords;

let banned = BannedPasswords::with_extra(["companyname2026", "product123"]);
assert!(banned.contains("P@ssw0rd")); // built-ins included, case-insensitive
```

The built-in list (~130 entries) includes composition-passing variants like
`P@ssw0rd` and `Password1!` — exactly the passwords that satisfy every
character-class rule and still fall to the first dictionary attack. It is a
last line of defense, not a real breached-password list; production
deployments should extend it from a file (see the guide).

## Generator

```rust
use pw::Generator;

let password = Generator::new(16)
    .uppercase()
    .lowercase()
    .digits()
    .special()
    .generate();
```

When `length >= enabled classes`, the output is guaranteed to contain at
least one character from every enabled class (one pick per class, filled from
the merged charset, Fisher–Yates shuffled). Panics if no class is enabled.
Backed by `rand::rng()` (a CSPRNG).

## API keys

```rust
let key = pw::generate_api_key();     // "sk_live_<43 chars base64url>" (256-bit)
let admin = pw::generate_admin_key(); // "ak_live_..."
let stored = pw::hash_api_key(&key);  // SHA-256 hex — store this, never the key
```

Keys are high-entropy random secrets, so plain SHA-256 (no salt/stretching)
is the appropriate storage hash — lookups stay O(1) by exact hash match.

## Tests

```bash
cargo test -p pw
```
