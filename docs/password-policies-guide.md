# Password Policies Guide

Stargate validates every password it accepts against a configurable policy:
composition rules (length, character classes), context rules (no username or
email inside the password), a banned-password list, and stateful rules
(expiry, reuse history). This guide explains how the pieces fit together and
how to configure them.

Crate-level API documentation lives in [`crates/pw/README.md`](../crates/pw/README.md).

## Where policies are enforced

Every path that accepts a new password validates it; there is no way to set a
password that skips the policy:

| Path | Policy applied |
|------|----------------|
| `PUT /signup` (self-registration) | Global |
| `POST /admin/users` (admin-created user) | Global |
| `stargate admin bootstrap` CLI | Global (user-supplied passwords only — `--generate-password` output is exempt: 40 random alphanumeric chars, ~238 bits) |
| `PUT /account/credentials` (password reset/change) | Global **or the user's organization override(s)**, plus the history rule |
| `POST /account/login` | Expiry rule only (`max_age_days`) |

A rejected password returns `400` with every violated rule joined into one
message:

```json
{
  "message": "Password must be at least 12 characters long. Password must contain at least one digit",
  "code": "password.policy_violation",
  "type": "validation",
  "link": "https://docs.stargate.dev/errors/password.policy_violation"
}
```

## Discovering the requirements: `GET /account/password-policy`

Public endpoint (no auth) that returns the global policy so signup/reset UIs
can render a live requirements checklist instead of hardcoding rules:

```json
{
  "minLength": 8,
  "maxLength": 64,
  "lowercase": true,
  "uppercase": true,
  "digits": true,
  "specialChars": true,
  "notUsername": true,
  "notEmail": true,
  "bannedPasswords": true,
  "regexPattern": null,
  "maxRepeats": null,
  "maxAgeDays": null,
  "history": null
}
```

Organization overrides are not reflected here (the caller may not be
authenticated); they are enforced server-side and surfaced through `400`
responses on submit.

## The rules

| Rule | Default | Rejects when |
|------|---------|--------------|
| `min_length` | 8 | Fewer than N characters (characters, not bytes — `ä` counts once) |
| `max_length` | 64 | More than N characters |
| `lowercase` | on | No lowercase letter |
| `uppercase` | on | No uppercase letter |
| `digits` | on | No digit |
| `special_chars` | on | No non-alphanumeric character |
| `not_username` | on | Password contains the account username/nickname (case-insensitive; usernames shorter than 3 chars are ignored) |
| `not_email` | on | Password contains the account email or its local part |
| `banned_passwords` | on | Password is on the banned list (case-insensitive) |
| `regex_pattern` | off | Password does not match the configured regex |
| `max_repeats` | off | Any run of the same character longer than N (`max_repeats: 2` rejects `aaa`) |
| `max_age_days` | off | *At login:* password older than N days (see [Expiry](#password-expiry-max_age_days)) |
| `history` | off | *On change:* password matches any of the last N passwords, current included (max 24) |

Regardless of configuration, passwords over **512 bytes** are always
rejected before hashing.

### Violation codes

Each rule has a stable machine code, useful for i18n or custom frontend
messaging (the API currently sends the joined human messages):

`too_many_bytes`, `too_short`, `too_long`, `missing_lowercase`,
`missing_uppercase`, `missing_digit`, `missing_special_char`,
`contains_username`, `contains_email`, `banned_password`,
`pattern_mismatch`, `repeated_characters`, `recently_used`.

## Configuring the global policy (env)

The global policy is the standard policy above overridden by
`PASSWORD_POLICY_*` env vars. Numeric vars treat `0` as "disable the rule";
boolean vars accept `1/true/yes/on`. Empty values count as unset. Invalid
values (bad number, uncompilable regex, min > max) **fail startup** with a
clear error rather than silently weakening the policy.

```env
# Length bounds (characters; 0 disables — the 512-byte cap still applies)
PASSWORD_POLICY_MIN_LENGTH=12
PASSWORD_POLICY_MAX_LENGTH=64

# Character classes
PASSWORD_POLICY_LOWERCASE=true
PASSWORD_POLICY_UPPERCASE=true
PASSWORD_POLICY_DIGITS=true
PASSWORD_POLICY_SPECIAL_CHARS=false

# Context rules
PASSWORD_POLICY_NOT_USERNAME=true
PASSWORD_POLICY_NOT_EMAIL=true

# Banned list
PASSWORD_POLICY_BANNED_PASSWORDS=true
PASSWORD_POLICY_BANNED_FILE=/etc/stargate/10k-most-common.txt

# Extras
PASSWORD_POLICY_REGEX=^[\x20-\x7E]+$
PASSWORD_POLICY_MAX_REPEATS=3

# Stateful rules
PASSWORD_POLICY_MAX_AGE_DAYS=90
PASSWORD_POLICY_HISTORY=5
```

The effective policy is logged at startup:

```
Password policy: min_length=Some(12) max_length=Some(64) classes[...] banned=true (N entries) ...
```

### The banned-password list

The built-in list is ~130 very common passwords, including
composition-passing variants (`P@ssw0rd`, `Password1!`, `Welcome123`). It is
intentionally small; NIST 800-63B expects checks against a real
common/breached-password corpus. Point `PASSWORD_POLICY_BANNED_FILE` at one —
one password per line, `#` lines are comments, matching is case-insensitive:

```
# top-10k list, downloaded 2026-07
123456
password
qwerty
...
```

The file is loaded once at startup and merged with the built-in list.

## Per-organization overrides

An organization can **replace** the global policy for its members by setting
`attrs.password_policy` (or `attrs.passwordPolicy`) on the organization:

```json
{
  "password_policy": {
    "min_length": 14,
    "special_chars": true,
    "max_age_days": 60,
    "history": 10
  }
}
```

Semantics:

- **Replace, not merge, against the global policy** — but fields omitted from
  the override take the *standard* defaults (min 8/max 64, all classes,
  context rules, banned list on). Rules must be disabled explicitly
  (`"digits": false`), so a partial override cannot silently weaken the
  baseline.
- Unknown fields are rejected; snake_case and camelCase are both accepted.
- An invalid override (typo, min > max, bad regex) is logged as a warning and
  ignored — the global policy applies instead. Nothing fails open.
- A user in **multiple** organizations with policies must satisfy **all** of
  them; for expiry the smallest `max_age_days` wins, for history the largest
  N wins.
- Organization overrides apply where the user is known: the password
  reset/change path and login expiry. Signup and admin user creation use the
  global policy (the user has no organization yet at that point).

## Password expiry (`max_age_days`)

Age is measured from the credential's last change (`credentials.updated_at`;
transparent hash upgrades do not touch it). When a password login succeeds
but the password is past its max age, Stargate does **not** issue session
tokens. Instead it returns `202` with a ready-to-use change-password token:

```json
{
  "passwordExpired": true,
  "message": "Password has expired and must be changed",
  "resetToken": "eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiIs...",
  "expiresInSecs": 3600
}
```

The client submits it to the normal reset endpoint:

```
PUT /account/credentials
{ "token": "<resetToken>", "password": "<new password>" }
```

…and then logs in again with the new password. No email round-trip is needed:
the user just proved knowledge of the (expired) password.

Ordering and scope:

- **MFA accounts**: the expiry check runs *after* MFA verification (on
  `PUT /account/login/mfa/challenges/{id}`), so an expired password alone
  never yields a reset token — password + OTP are still both required.
- **Passwordless email-OTP login** is not gated (no password is involved),
  and **refresh tokens** keep working for existing sessions; expiry is
  enforced at password login.
- If a pending change-password request already exists (e.g. from the forgot
  flow), its token stays valid — the login response reuses it.

## Password history (`history`)

With `history: n`, a password change is rejected when the new password
matches the current password or any of the previous `n − 1`. Previous hashes
live in the `credential_history` table, appended and pruned (cap 24) in the
same transaction as the password change. The check runs only after all other
rules pass — each comparison is a full Argon2 verification — and rejects with:

```
Password was used recently; choose one you have not used before
```

History applies on `PUT /account/credentials`. It is not retroactive:
entries accumulate from the first password change after the feature is
enabled.

## Hashing configuration (Argon2 + pepper)

Stored hashes are Argon2id in PHC format. Costs and an optional pepper come
from env, validated at startup:

```env
ARGON2_MEMORY_KIB=19456   # default; raise for stronger hashes
ARGON2_ITERATIONS=2
ARGON2_PARALLELISM=1
PASSWORD_PEPPER=<openssl rand -hex 32>
```

The pepper is applied as the Argon2 *secret* (keyed hashing): a database dump
alone is not enough to attack peppered hashes offline.

**Transparent upgrades.** When costs change or a pepper is introduced,
existing hashes keep verifying; on the next successful login the hash is
re-encoded in the background with the current settings (audited as a
credential update with `{"credential_rehash": true}`, without touching the
expiry timestamp or history). No user action, no forced resets.

Operational cautions:

- Keep the pepper **outside the database** (env/secret manager) and back it
  up — a lost pepper invalidates every peppered hash.
- Introducing a pepper on an existing deployment is safe (fallback + rehash).
  **Rotating** an existing pepper is not supported: hashes created with the
  old pepper will stop verifying. Plan rotation as a password-reset event.
- In cluster mode, all nodes must share the same `PASSWORD_PEPPER` and should
  share the same `ARGON2_*` values (mismatched costs cause rehash churn as
  logins bounce between nodes).

## Recipes

**Length-only policy (NIST-style, no composition rules):**

```env
PASSWORD_POLICY_MIN_LENGTH=12
PASSWORD_POLICY_LOWERCASE=false
PASSWORD_POLICY_UPPERCASE=false
PASSWORD_POLICY_DIGITS=false
PASSWORD_POLICY_SPECIAL_CHARS=false
PASSWORD_POLICY_BANNED_FILE=/etc/stargate/10k-most-common.txt
```

**Compliance-heavy tenant, stricter than the platform default** — set on the
organization:

```json
{
  "password_policy": {
    "min_length": 14,
    "max_age_days": 60,
    "history": 12,
    "max_repeats": 2
  }
}
```

**ASCII-only passwords** (some legacy downstream can't handle unicode):

```env
PASSWORD_POLICY_REGEX=^[\x20-\x7E]+$
```
