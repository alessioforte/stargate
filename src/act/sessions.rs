//! Per-user session index and org context selection.
//!
//! The index tracks every live `sid` a user holds so revocation flows can
//! kill sessions immediately (user delete, org-membership removal) instead
//! of waiting for token expiry. Layout: store hash
//! `account:sessions:{user_id}` with field = sid and a small JSON entry
//! recording the session's active org. Fields carry their own TTL matching
//! the refresh-token TTL so the index self-prunes; entries are also pruned
//! lazily when scanned.
//!
//! Deleting a `sid` record kills the session completely: the gateway and
//! account endpoints resolve subjects from the store by sid, and the
//! refresh flow requires the sid record to rotate — a cryptographically
//! valid JWT with no session behind it is rejected everywhere.

use crate::err::{ErrorResponse, HttpError};
use crate::etc::jwt::jwt_config;
use crate::etc::store::use_store;
use crate::etc::sub::{Subject, SubjectType};
use db::ent::OrgMembership;
use serde::{Deserialize, Serialize};
use store::{Store, StoreError};

const SESSION_INDEX_PREFIX: &str = "account:sessions";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionIndexEntry {
    pub org_id: Option<String>,
    /// Unix seconds; belt-and-braces expiry used for lazy pruning in stores
    /// where hash fields outlive their intended TTL.
    pub expires_at_unix: u64,
}

fn index_key(user_id: &str) -> String {
    format!("{SESSION_INDEX_PREFIX}:{user_id}")
}

fn now_unix() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

/// Record a session under the user's index. Failures are logged, not fatal:
/// the index is a revocation aid, and sessions still expire by TTL.
pub async fn register_session(user_id: &str, sid: &str, org_id: Option<&str>, ttl_secs: u64) {
    let entry = SessionIndexEntry {
        org_id: org_id.map(str::to_string),
        expires_at_unix: now_unix() + ttl_secs,
    };
    let store = use_store();
    if let Err(error) = store
        .hset(&index_key(user_id), sid, &entry, Some(ttl_secs))
        .await
    {
        tracing::warn!(user_id, sid, "failed to register session in index: {error}");
    }
}

/// Drop one sid from the index (logout, refresh rotation). The sid record
/// itself is deleted by the caller.
pub async fn forget_session(user_id: &str, sid: &str) {
    let store = use_store();
    if let Err(error) = store.hdel(&index_key(user_id), sid).await {
        tracing::warn!(user_id, sid, "failed to remove session from index: {error}");
    }
}

/// Delete every live session of the user and the index itself. Returns the
/// number of session records actually deleted.
pub async fn revoke_all_sessions(user_id: &str) -> Result<usize, StoreError> {
    let store = use_store();
    let key = index_key(user_id);
    let sids = store.hkeys(&key).await?;
    let mut revoked = 0;
    for sid in &sids {
        if store.delete(sid).await? {
            revoked += 1;
        }
    }
    store.delete(&key).await?;
    Ok(revoked)
}

/// Delete the user's sessions whose active org is `org_id`; org-less and
/// other-org sessions survive. Lazily prunes expired index entries. Returns
/// the number of session records actually deleted.
pub async fn revoke_org_sessions(user_id: &str, org_id: &str) -> Result<usize, StoreError> {
    let store = use_store();
    let key = index_key(user_id);
    let entries = store.hgetall::<SessionIndexEntry>(&key).await?;
    let now = now_unix();
    let mut revoked = 0;
    for (sid, entry) in entries {
        let expired = entry.expires_at_unix <= now;
        let matches = entry.org_id.as_deref() == Some(org_id);
        if matches && !expired && store.delete(&sid).await? {
            revoked += 1;
        }
        if matches || expired {
            let _ = store.hdel(&key, &sid).await;
        }
    }
    Ok(revoked)
}

// ── Session authentication ───────────────────────────────────────────────────

/// A fully validated user session: access token verified, not revoked, and
/// backed by a live sid record in the store.
pub struct AuthenticatedSession {
    pub user: db::ent::User,
    pub subject: Subject,
    pub auth_time: Option<usize>,
}

pub async fn authenticated_session(
    token: Option<String>,
) -> Result<AuthenticatedSession, ErrorResponse> {
    let invalid = || ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string()));

    let token = token.ok_or_else(|| {
        ErrorResponse::from(HttpError::Unauthorized("Token not found".to_string()))
    })?;

    let claims = jwt_config()
        .validate_session_access_token(&token)
        .map_err(|_| invalid())?;

    if crate::act::token_revocation::is_revoked(&claims)
        .await
        .map_err(ErrorResponse::internal)?
    {
        return Err(invalid());
    }

    let sid = claims.sid.clone().ok_or_else(invalid)?;
    let subject = use_store()
        .get::<Subject>(&sid)
        .await
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;

    if subject.sub_type != SubjectType::User {
        return Err(ErrorResponse::from(HttpError::Forbidden(
            "only available for user sessions".to_string(),
        )));
    }

    if claims.sub_id.as_deref() != Some(&subject.id) {
        return Err(invalid());
    }

    let user = crate::db::get_user_by_id(&subject.id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(invalid)?;

    Ok(AuthenticatedSession {
        user,
        subject,
        auth_time: claims.auth_time,
    })
}

// ── Org context selection ────────────────────────────────────────────────────

/// The org context a session acts in.
#[derive(Debug, Clone)]
pub struct OrgContext {
    pub org_id: String,
    pub role: String,
}

impl From<&OrgMembership> for OrgContext {
    fn from(membership: &OrgMembership) -> Self {
        OrgContext {
            org_id: membership.organization.id.clone(),
            role: membership.role.clone(),
        }
    }
}

/// Resolve the org context for a new session:
///
/// 1. explicitly requested org — must be a membership, otherwise the login
///    is rejected;
/// 2. `users.attrs.default_org` — skipped silently when stale;
/// 3. the oldest membership;
/// 4. none (org-less session).
pub async fn resolve_org_context(
    user: &db::ent::User,
    requested_org_id: Option<&str>,
) -> Result<Option<OrgContext>, ErrorResponse> {
    let memberships = crate::db::get_user_organizations(&user.id)
        .await
        .map_err(ErrorResponse::internal)?;
    let default_org = user
        .attrs
        .get("default_org")
        .or_else(|| user.attrs.get("defaultOrg"))
        .and_then(|value| value.as_str());

    select_org(&memberships, requested_org_id, default_org)
        .map(|selected| selected.map(OrgContext::from))
        .map_err(|_| {
            ErrorResponse::from(HttpError::BadRequest(
                "not a member of the requested organization".to_string(),
            ))
        })
}

#[derive(Debug)]
pub struct NotAMember;

/// Pure selection over the membership list; see `resolve_org_context` for
/// the precedence rules.
fn select_org<'a>(
    memberships: &'a [OrgMembership],
    requested: Option<&str>,
    default_org: Option<&str>,
) -> Result<Option<&'a OrgMembership>, NotAMember> {
    let find = |org_id: &str| {
        memberships
            .iter()
            .find(|membership| membership.organization.id == org_id)
    };

    if let Some(requested) = requested {
        return find(requested).map(Some).ok_or(NotAMember);
    }

    if let Some(default_org) = default_org
        && let Some(membership) = find(default_org)
    {
        return Ok(Some(membership));
    }

    // Oldest membership; entries backfilled without a timestamp sort first.
    Ok(memberships
        .iter()
        .min_by_key(|membership| membership.member_since))
}

#[cfg(all(test, feature = "memory"))]
mod index_tests {
    use super::*;
    use serde_json::json;

    async fn seed_session(user_id: &str, sid: &str, org: Option<&str>) {
        use_store()
            .set(sid, &json!({ "id": user_id }), Some(60))
            .await
            .unwrap();
        register_session(user_id, sid, org, 60).await;
    }

    fn unique(prefix: &str) -> String {
        format!("{prefix}-{}", ulid::Ulid::new())
    }

    #[tokio::test]
    async fn revoke_all_sessions_deletes_every_sid_and_the_index() {
        let user = unique("user");
        let sid_a = unique("sid");
        let sid_b = unique("sid");
        seed_session(&user, &sid_a, None).await;
        seed_session(&user, &sid_b, Some("org_a")).await;

        let revoked = revoke_all_sessions(&user).await.unwrap();

        assert_eq!(revoked, 2);
        assert!(!use_store().exists(&sid_a).await.unwrap());
        assert!(!use_store().exists(&sid_b).await.unwrap());
        // Idempotent: nothing left to revoke.
        assert_eq!(revoke_all_sessions(&user).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn revoke_org_sessions_spares_other_contexts() {
        let user = unique("user");
        let sid_target = unique("sid");
        let sid_other_org = unique("sid");
        let sid_orgless = unique("sid");
        seed_session(&user, &sid_target, Some("org_a")).await;
        seed_session(&user, &sid_other_org, Some("org_b")).await;
        seed_session(&user, &sid_orgless, None).await;

        let revoked = revoke_org_sessions(&user, "org_a").await.unwrap();

        assert_eq!(revoked, 1);
        assert!(!use_store().exists(&sid_target).await.unwrap());
        assert!(use_store().exists(&sid_other_org).await.unwrap());
        assert!(use_store().exists(&sid_orgless).await.unwrap());
    }

    #[tokio::test]
    async fn forget_session_removes_the_index_entry_but_not_the_record() {
        let user = unique("user");
        let sid = unique("sid");
        seed_session(&user, &sid, None).await;

        forget_session(&user, &sid).await;

        // The record is the caller's to delete; only the index entry is gone.
        assert!(use_store().exists(&sid).await.unwrap());
        assert_eq!(revoke_all_sessions(&user).await.unwrap(), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use db::ent::Organization;

    fn membership(org_id: &str, role: &str, member_since_unix: Option<i64>) -> OrgMembership {
        let mut organization = Organization::new(org_id.to_string(), None);
        organization.id = org_id.to_string();
        OrgMembership {
            organization,
            role: role.to_string(),
            member_since: member_since_unix.map(|secs| Utc.timestamp_opt(secs, 0).unwrap()),
        }
    }

    #[test]
    fn requested_org_wins_and_must_be_a_membership() {
        let memberships = vec![
            membership("org_a", "member", Some(100)),
            membership("org_b", "admin", Some(200)),
        ];

        let selected = select_org(&memberships, Some("org_b"), Some("org_a"))
            .unwrap()
            .unwrap();
        assert_eq!(selected.organization.id, "org_b");

        assert!(select_org(&memberships, Some("org_x"), None).is_err());
    }

    #[test]
    fn stale_default_org_falls_back_to_oldest_membership() {
        let memberships = vec![
            membership("org_b", "admin", Some(200)),
            membership("org_a", "member", Some(100)),
        ];

        let selected = select_org(&memberships, None, Some("org_gone"))
            .unwrap()
            .unwrap();
        assert_eq!(selected.organization.id, "org_a");
    }

    #[test]
    fn default_org_beats_oldest_membership() {
        let memberships = vec![
            membership("org_a", "member", Some(100)),
            membership("org_b", "admin", Some(200)),
        ];

        let selected = select_org(&memberships, None, Some("org_b"))
            .unwrap()
            .unwrap();
        assert_eq!(selected.organization.id, "org_b");
    }

    #[test]
    fn no_memberships_selects_none() {
        assert!(select_org(&[], None, None).unwrap().is_none());
        assert!(select_org(&[], None, Some("org_a")).unwrap().is_none());
    }

    #[test]
    fn backfilled_membership_without_timestamp_counts_as_oldest() {
        let memberships = vec![
            membership("org_a", "member", Some(100)),
            membership("org_b", "admin", None),
        ];

        let selected = select_org(&memberships, None, None).unwrap().unwrap();
        assert_eq!(selected.organization.id, "org_b");
    }
}
