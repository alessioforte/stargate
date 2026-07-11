# `crates/pw` — Password Module Improvement Plan

Plan for improving and extending `crates/pw` (hashing, password policies,
generation, API keys), with a focus on password policies.

## Status

| Phase | Scope | Status |
|-------|-------|--------|
| 1 | Close enforcement gaps & correctness fixes | ✅ Completed (2026-07-11) |
| 2 | Configurable, typed policy engine | ✅ Completed (2026-07-11) |
| 3 | Stateful policies (expiry, history) | ✅ Completed (2026-07-11) |
| 4 | Hashing hardening + cleanup | ✅ Completed (2026-07-11) |

> Update this table and the per-phase checklists as work completes.

## Findings (2026-07-11 analysis)

1. **Policy only enforced at signup.** `PasswordPolicy::standard().validate()`
   runs only in `PUT /signup`. Password reset (`PUT /account/credentials`),
   admin user creation (`POST /admin/users`), and CLI bootstrap accept any
   password — the signup policy is trivially bypassed by resetting.
2. **Policy model can't carry configuration.** `parse("min_length")` always
   yields 8; the string form loses parameters. Unknown rule names are silently
   dropped (`filter_map`), so config typos weaken security without error. No
   serde, so policies can't come from env/config/org attrs.
3. **Length rules use byte length.** `password.len()` counts UTF-8 bytes, not
   characters; multibyte passphrases hit `max_length` early and get extra
   credit toward `min_length`. No hard byte cap before Argon2 hashing.
4. **Errors are bare `String`s, first-failure-only.** No stable codes for
   i18n, no aggregated violation list for a frontend requirements checklist.
5. **Generator can violate its own policy.** Uniform sampling from the merged
   charset doesn't guarantee one char per enabled class. Positional-bool API
   (`generator(512, false, true, true, false)`) is unreadable at call sites.
6. **Hashing fixed to `Argon2::default()`.** Good params today, but not
   tunable, no pepper (despite `EMAIL_OTP_PEPPER` precedent), no
   `needs_rehash` upgrade path.
7. **Minor:** `keys/parse.rs` is dead code; API-key format base64-encodes an
   already-alphanumeric string; `docs/AGENTS.md` describes the crate as
   hashing-only.

---

## Phase 1 — Close enforcement gaps & correctness fixes

- [x] Add `crate::etc::pw::validate_password()` as the single choke point for
      password acceptance (wraps `PasswordPolicy::standard()` for now; Phase 2
      swaps in the configurable effective policy here).
- [x] Enforce policy in `PUT /account/credentials` (reset), `POST /admin/users`
      (create), CLI `admin bootstrap` (user-supplied `--password` /
      `--password-stdin` only), and refactor signup to use the choke point.
- [x] Switch `min_length`/`max_length` to `chars().count()`.
- [x] Add unconditional `MAX_PASSWORD_BYTES` (512) pre-check in `validate()`.
- [x] `PasswordPolicy::new`/`parse` return `Result`, failing on unknown names.
- [x] Replace `generator()` with a `Generator` builder that guarantees ≥1 char
      per enabled class (place one per class + fill + Fisher–Yates shuffle),
      O(1) charset indexing; update call sites (`src/etc/jwt.rs`,
      `src/cli/mod.rs`).
- [x] Unit tests for all of the above; workspace builds clean.

Completed 2026-07-11. Notes:
- 17 unit tests + 2 doctests pass (`cargo test -p pw`); both `--features edge`
  and `--features cluster` type-check clean.
- Verified end-to-end: `admin bootstrap --password weak` is rejected with
  "Password must be at least 8 characters long".
- Fixed a latent build issue found along the way: `pw` relied on other
  workspace crates to enable `rand_core/getrandom` for `OsRng`; it now
  declares `password-hash = { features = ["getrandom"] }` itself, so
  `cargo test -p pw` builds standalone.
- `PUT /account/credentials` now documents a 400 response in OpenAPI.

Decisions:
- CLI `--generate-password` output (40-char upper+lower+digits) is **exempt**
  from composition validation: at ~238 bits of entropy it is categorically
  stronger than any composition rule, and omitting specials keeps it
  copy/paste-safe. Only user-supplied passwords are validated.
- Existing error message strings are kept verbatim in Phase 1; typed errors
  with stable codes land in Phase 2.

## Phase 2 — Configurable, typed policy engine

- [x] `PasswordPolicySet` struct: serde-derived, parameterized rules
      (`min_length: 12`), replaces `Vec<PasswordPolicy>`.
- [x] Typed `PolicyViolation` error enum with stable codes;
      `validate_all()` returns every violation (enables i18n + live
      requirements checklist in `apps/auth`).
- [x] Context-aware rules: `not_username` / `not_email` via
      `PolicyContext { username, email }` passed to validate.
- [x] `banned_passwords`: embedded common-password list (~130 entries,
      including composition-passing variants like `P@ssw0rd`) + optional
      file-driven list via `PASSWORD_POLICY_BANNED_FILE`.
- [x] `regex_pattern` rule (`regex` dependency, `is_match` semantics).
- [x] `max_repeats` rule (max run of the same character). Sequential-char
      and `zxcvbn` min-score rules **deferred** (revisit after Phase 3).
- [x] App-layer config plumbing mirroring the MFA pattern:
      `PASSWORD_POLICY_*` env vars for the global default, per-org override
      via `organizations.attrs.password_policy`, effective-policy resolution
      in `src/act/password_policy.rs`.
- [x] `GET /account/password-policy` so frontends render requirements instead
      of hardcoding them.

Completed 2026-07-11. Semantics decided during implementation:
- Serde: snake_case canonical, camelCase aliases accepted,
  `deny_unknown_fields`; missing fields take `standard()` values so partial
  configs strengthen rather than silently weaken; rules disabled only by
  explicit `false`/`null`/`0`.
- Org override **replaces** the global policy entirely (no per-field merge)
  and applies on the reset path where the user is known; a user in multiple
  policy-bearing orgs must satisfy all of them. Signup, admin create, and CLI
  use the global policy. Malformed/invalid org policies are logged and
  ignored (fail to global).
- `PasswordPolicySet::check()` validates policy self-consistency (min<=max,
  regex compiles, max_repeats>=1); `act::password_policy::init()` runs it at
  boot and panics on bad `PASSWORD_POLICY_*` env. Empty env values count as
  unset. Invalid regex at validate time is skipped (composition rules still
  apply).
- 400 responses join all violation messages with `. `; stable machine codes
  exist on `PolicyViolation::code()` but the error envelope keeps its
  single-`message` shape for now.
- Verified live: `GET /account/password-policy` returns the env-configured
  policy; banned ("Password1!") and contains-username/email passwords are
  rejected via the CLI path with aggregated messages.

## Phase 3 — Stateful policies (app + db layer)

- [x] **Expiry / max auth age**: reuses `credentials.updated_at` (its only
      mutator is `change_password`); an expired password at login returns a
      `202 passwordExpired` response carrying a change-password `resetToken`
      that feeds the existing `PUT /account/credentials` flow.
- [x] **History (`history: n`)**: `credential_history` table keeping previous
      Argon2 hashes per user (pruned to 24); on change, candidate is verified
      against the current hash + last *n−1* history entries. Crate exposes
      `Hash::matches_any`; storage lives in `crates/db` inside the same
      transaction as the password change (audit-context preserved).

Completed 2026-07-11. Semantics decided during implementation:
- New policy fields `max_age_days` / `history` on `PasswordPolicySet` (env:
  `PASSWORD_POLICY_MAX_AGE_DAYS` / `PASSWORD_POLICY_HISTORY`, org-overridable,
  exposed on `GET /account/password-policy`). They are **stateful**: never
  evaluated by `validate_all`, enforced by `src/act/password_policy.rs`
  (`password_expired`, history check inside `validate_for_user`).
- Expiry check runs at token issuance for password logins only: plain login
  after password verify, and the login-MFA verify endpoint after OTP — so an
  expired password alone never yields a reset token for an MFA account.
  Passwordless email-OTP login and refresh tokens are intentionally not
  gated.
- The 202 response reuses a pending change-password request sid when one
  exists (keeps emailed reset links valid), else creates one
  (`expiresInSecs` = 1h TTL upper bound).
- Multiple orgs: strictest wins — smallest `max_age_days`, largest `history`.
- History check runs only when the password passes all string rules (each
  comparison is a full Argon2 verify on the blocking pool); DB read errors
  fail towards "not used" with a warning.
- New migration `20260711000000_credential_history.sql` (both backends);
  history append + prune happen in the same tx as the password update.
- Verified live end-to-end (fresh edge instance): fresh login 200 → backdated
  credential → login 202 with `resetToken` → PUT reset 200 → history row
  written → reuse of original or current password rejected 400
  "used recently" → fresh password accepted; 23 crate + 118 binary tests
  pass, both profiles compile.

## Phase 4 — Hashing hardening

- [x] Env-tunable Argon2 params (`ARGON2_MEMORY_KIB`, `ARGON2_ITERATIONS`,
      `ARGON2_PARALLELISM`), validated at boot via `Hash::configure`
      (`etc::pw::init()`, called before CLI dispatch so `admin bootstrap`
      hashes identically to the server).
- [x] Optional `PASSWORD_PEPPER`, applied as the Argon2 `secret` (keyed
      hashing). Migration-safe: when the peppered check fails, verification
      falls back to unpeppered `Argon2::default()` and reports
      `needs_rehash`, so pre-pepper hashes keep working and upgrade on the
      next login.
- [x] `Hash::verify_for_login` / `needs_rehash` comparing stored PHC params
      (algorithm + m/t/p) against current config; transparent
      **rehash-on-login**: background task re-encodes and stores via
      `rehash_credential` — a value-guarded UPDATE (concurrent-login safe)
      that preserves `updated_at` (expiry timer) and skips history, audited
      with `{"credential_rehash": true}` metadata.

Completed 2026-07-11. Verified live: bootstrap with defaults (`m=19456`,
unpeppered) → server restarted with `PASSWORD_PEPPER` + `ARGON2_MEMORY_KIB=32768`
→ login 200 via fallback → stored hash upgraded in place to `m=32768`
(peppered), `updated_at` unchanged, no history row, audit row written →
subsequent login verifies against the peppered hash directly; wrong password
still 401. 28 crate + 118 binary tests pass, both profiles compile.

## Cleanup (opportunistic, any phase)

- [x] Deleted dead `keys/parse.rs`.
- [x] API-key generation now emits 32 random bytes base64url-encoded
      (256 bits, 43 chars), keeping the `sk_live_` prefix format.
- [x] Updated `docs/AGENTS.md` + `.claude/CLAUDE.md`: `crates/pw` description
      and env-table rows for `PASSWORD_POLICY_*`, `ARGON2_*`,
      `PASSWORD_PEPPER`; full var docs in `.env.example`.

---

**All four phases complete (2026-07-11).** Deferred ideas for future work:
`zxcvbn` strength scoring behind a feature flag, sequential-character rule,
structured violation arrays in the error envelope, per-org policy on the
signup path (needs an org context at signup), and admin APIs to inspect or
clear a user's password history.
