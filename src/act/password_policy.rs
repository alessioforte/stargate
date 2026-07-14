//! Effective password-policy resolution.
//!
//! The global policy is `PasswordPolicySet::standard()` overridden by
//! `PASSWORD_POLICY_*` env vars, with an optional extended banned-password
//! list from `PASSWORD_POLICY_BANNED_FILE`. Organizations can replace the
//! global policy for their members via `attrs.password_policy` (validated;
//! invalid overrides are logged and ignored). Every path that accepts a new
//! password validates through this module: signup, password reset, admin
//! user creation, and the CLI. Roadmap: `docs/pw-improvement-plan.md`.

use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use db::ent::{CredentialType, Organization, User};
use pw::{BannedPasswords, PasswordPolicySet, PolicyContext, PolicyViolation};

use crate::etc::env;

pub struct GlobalPasswordPolicy {
    pub set: PasswordPolicySet,
    banned: BannedPasswords,
}

static GLOBAL: LazyLock<GlobalPasswordPolicy> = LazyLock::new(load_global);

pub fn global() -> &'static GlobalPasswordPolicy {
    &GLOBAL
}

/// Force the global policy to load at boot so `PASSWORD_POLICY_*`
/// misconfiguration fails fast with a clear message instead of erroring on
/// the first signup.
pub fn init() {
    let global = global();
    tracing::info!(
        "Password policy: min_length={:?} max_length={:?} classes[lower={} upper={} digit={} special={}] not_username={} not_email={} banned={} ({} entries) regex={} max_repeats={:?} max_age_days={:?} history={:?}",
        global.set.min_length,
        global.set.max_length,
        global.set.lowercase,
        global.set.uppercase,
        global.set.digits,
        global.set.special_chars,
        global.set.not_username,
        global.set.not_email,
        global.set.banned_passwords,
        global.banned.len(),
        global.set.regex_pattern.is_some(),
        global.set.max_repeats,
        global.set.max_age_days,
        global.set.history,
    );
}

/// Validate a candidate password against the global policy. Use for accounts
/// with no organization context yet (signup, admin create, CLI bootstrap).
pub fn validate_global(
    password: &str,
    username: Option<&str>,
    email: Option<&str>,
) -> Result<(), String> {
    let global = global();
    let ctx = PolicyContext {
        username,
        email,
        banned: Some(&global.banned),
    };
    to_result(global.set.validate_all(password, &ctx))
}

/// Validate a candidate password for an existing user. Organizations the
/// user belongs to replace the global policy via `attrs.password_policy`;
/// the password must satisfy every applicable organization policy.
pub async fn validate_for_user(user: &User, password: &str) -> Result<(), String> {
    let global = global();
    let ctx = PolicyContext {
        username: Some(&user.nickname),
        email: Some(&user.email),
        banned: Some(&global.banned),
    };

    let org_sets = org_policies_for_user(&user.id).await;
    let sets: &[PasswordPolicySet] = if org_sets.is_empty() {
        std::slice::from_ref(&global.set)
    } else {
        &org_sets
    };

    let mut violations: Vec<PolicyViolation> = Vec::new();
    for set in sets {
        for violation in set.validate_all(password, &ctx) {
            if !violations.contains(&violation) {
                violations.push(violation);
            }
        }
    }

    // Stateful history rule, only checked once the password is otherwise
    // acceptable: each comparison is a full Argon2 verification.
    if violations.is_empty() {
        let depth = sets.iter().filter_map(|set| set.history).max().unwrap_or(0);
        if depth > 0 && recently_used(&user.id, password, depth).await {
            violations.push(PolicyViolation::RecentlyUsed);
        }
    }

    to_result(violations)
}

/// Whether `password` matches the user's current password or any of their
/// last `depth - 1` previous ones. DB errors fail towards "not used" with a
/// warning — the change itself will surface persistent DB failures.
async fn recently_used(user_id: &str, password: &str, depth: usize) -> bool {
    let mut hashes: Vec<String> = Vec::with_capacity(depth);

    match crate::db::get_credential(user_id, CredentialType::Password).await {
        Ok(Some(current)) => hashes.push(current.value),
        Ok(None) => {}
        Err(error) => {
            tracing::warn!("could not load credential for password history check: {error}");
        }
    }
    if depth > 1 {
        match crate::db::get_credential_history(user_id, (depth - 1) as i64).await {
            Ok(entries) => hashes.extend(entries.into_iter().map(|entry| entry.value)),
            Err(error) => {
                tracing::warn!("could not load password history, checking current only: {error}");
            }
        }
    }
    if hashes.is_empty() {
        return false;
    }

    let password = password.to_string();
    match tokio::task::spawn_blocking(move || {
        pw::Hash::matches_any(&password, hashes.iter().map(String::as_str))
    })
    .await
    {
        Ok(matched) => matched,
        Err(error) => {
            tracing::error!("password history check task failed: {error}");
            false
        }
    }
}

/// Whether the user's password is older than the effective `max_age_days`.
/// Organization policies replace the global one; with multiple organizations
/// the strictest (smallest) configured age wins. `false` when expiry is not
/// configured anywhere.
pub async fn password_expired(user_id: &str, changed_at: DateTime<Utc>) -> bool {
    let org_sets = org_policies_for_user(user_id).await;
    let max_age_days = if org_sets.is_empty() {
        global().set.max_age_days
    } else {
        org_sets.iter().filter_map(|set| set.max_age_days).min()
    };
    match max_age_days {
        Some(days) => Utc::now() - changed_at > chrono::Duration::days(days as i64),
        None => false,
    }
}

fn to_result(violations: Vec<PolicyViolation>) -> Result<(), String> {
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations
            .iter()
            .map(|violation| violation.to_string())
            .collect::<Vec<_>>()
            .join(". "))
    }
}

async fn org_policies_for_user(user_id: &str) -> Vec<PasswordPolicySet> {
    let organizations = match crate::db::get_user_organizations(user_id).await {
        Ok(organizations) => organizations,
        Err(error) => {
            // Fail towards the global policy: the password is still fully
            // validated, only org-specific overrides are skipped.
            tracing::warn!(
                "could not load organizations for password policy, using global policy: {error}"
            );
            return Vec::new();
        }
    };
    organizations
        .iter()
        .filter_map(|membership| org_policy(&membership.organization))
        .collect()
}

fn org_policy(organization: &Organization) -> Option<PasswordPolicySet> {
    let value = organization
        .attrs
        .get("password_policy")
        .or_else(|| organization.attrs.get("passwordPolicy"))?;
    if value.is_null() {
        return None;
    }

    let set = match serde_json::from_value::<PasswordPolicySet>(value.clone()) {
        Ok(set) => set,
        Err(error) => {
            tracing::warn!(
                "organization {} has malformed attrs.password_policy, using global policy: {error}",
                organization.id
            );
            return None;
        }
    };
    match set.check() {
        Ok(()) => Some(set),
        Err(error) => {
            tracing::warn!(
                "organization {} has invalid attrs.password_policy, using global policy: {error}",
                organization.id
            );
            None
        }
    }
}

fn load_global() -> GlobalPasswordPolicy {
    let standard = PasswordPolicySet::standard();
    let set = PasswordPolicySet {
        min_length: optional_usize("PASSWORD_POLICY_MIN_LENGTH", standard.min_length),
        max_length: optional_usize("PASSWORD_POLICY_MAX_LENGTH", standard.max_length),
        lowercase: env::bool_or("PASSWORD_POLICY_LOWERCASE", standard.lowercase),
        uppercase: env::bool_or("PASSWORD_POLICY_UPPERCASE", standard.uppercase),
        digits: env::bool_or("PASSWORD_POLICY_DIGITS", standard.digits),
        special_chars: env::bool_or("PASSWORD_POLICY_SPECIAL_CHARS", standard.special_chars),
        not_username: env::bool_or("PASSWORD_POLICY_NOT_USERNAME", standard.not_username),
        not_email: env::bool_or("PASSWORD_POLICY_NOT_EMAIL", standard.not_email),
        banned_passwords: env::bool_or(
            "PASSWORD_POLICY_BANNED_PASSWORDS",
            standard.banned_passwords,
        ),
        regex_pattern: nonempty_string("PASSWORD_POLICY_REGEX"),
        max_repeats: optional_usize("PASSWORD_POLICY_MAX_REPEATS", None),
        max_age_days: optional_usize("PASSWORD_POLICY_MAX_AGE_DAYS", None).map(|days| days as u32),
        history: optional_usize("PASSWORD_POLICY_HISTORY", None),
    };
    if let Err(error) = set.check() {
        panic!("invalid PASSWORD_POLICY_* configuration: {error}");
    }

    let banned = match nonempty_string("PASSWORD_POLICY_BANNED_FILE") {
        Some(path) => {
            let content = std::fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!("failed to read PASSWORD_POLICY_BANNED_FILE `{path}`: {error}")
            });
            BannedPasswords::with_extra(
                content
                    .lines()
                    .filter(|line| !line.trim_start().starts_with('#')),
            )
        }
        None => BannedPasswords::builtin(),
    };

    GlobalPasswordPolicy { set, banned }
}

/// Read an optional string env var, treating empty/blank as unset.
fn nonempty_string(name: &str) -> Option<String> {
    env::optional_string(name).filter(|value| !value.trim().is_empty())
}

/// Read an optional non-negative integer env var; `0` disables the rule and
/// empty/blank counts as unset.
fn optional_usize(name: &str, default: Option<usize>) -> Option<usize> {
    match nonempty_string(name) {
        None => default,
        Some(raw) => match raw.trim().parse::<usize>() {
            Ok(0) => None,
            Ok(value) => Some(value),
            Err(error) => panic!("invalid {name} `{raw}`: {error}"),
        },
    }
}
