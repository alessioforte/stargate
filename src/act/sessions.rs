//! Stable login-session registry and org context selection.
//!
//! Each login has one stable standard JWT `sid`. Refresh-token replay
//! protection is tracked separately through a rotating JWT `jti`. The
//! per-user index supports targeted cleanup and the global directory supports
//! admin listing and aggregate counts. Hash fields carry their own TTL matching
//! the refresh-token TTL.
//!
//! Deleting a `sid` record kills the session completely: the gateway and
//! account endpoints resolve subjects from the store by sid, and the
//! refresh flow requires the sid record to rotate — a cryptographically
//! valid JWT with no session behind it is rejected everywhere.

use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::jwt::jwt_config;
use crate::etc::store::use_store;
use crate::etc::sub::{Subject, SubjectType};
use db::ent::OrgMembership;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use store::{Store, StoreError};

const SESSION_INDEX_PREFIX: &str = "account:sessions";
const SESSION_DIRECTORY_KEY: &str = "account:sessions:directory";
const REFRESH_ROTATION_PREFIX: &str = "account:session-refresh";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionIndexEntry {
    pub org_id: Option<String>,
    /// Unix seconds; belt-and-braces expiry used for lazy pruning in stores
    /// where hash fields outlive their intended TTL.
    pub expires_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    pub user_id: String,
    pub org_id: Option<String>,
    pub authenticated_at_unix: u64,
    pub last_seen_at_unix: u64,
    pub expires_at_unix: u64,
    #[serde(default)]
    pub client_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSummary {
    pub active_sessions: usize,
    pub active_users: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RefreshRotation {
    current_jti: String,
}

fn index_key(user_id: &str) -> String {
    format!("{SESSION_INDEX_PREFIX}:{user_id}")
}

fn refresh_rotation_key(sid: &str) -> String {
    format!("{REFRESH_ROTATION_PREFIX}:{sid}")
}

fn now_unix() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

fn ttl_until(expires_at_unix: u64) -> Option<u64> {
    let now = now_unix();
    (expires_at_unix > now).then_some(expires_at_unix - now)
}

fn index_entry(record: &SessionRecord) -> SessionIndexEntry {
    SessionIndexEntry {
        org_id: record.org_id.clone(),
        expires_at_unix: record.expires_at_unix,
    }
}

async fn store_record(record: &SessionRecord, ttl_secs: u64) -> Result<(), StoreError> {
    let store = use_store();
    let user_key = index_key(&record.user_id);
    store
        .hset(&user_key, &record.id, &index_entry(record), Some(ttl_secs))
        .await?;
    if let Err(error) = store
        .hset(SESSION_DIRECTORY_KEY, &record.id, record, Some(ttl_secs))
        .await
    {
        let _ = store.hdel(&user_key, &record.id).await;
        return Err(error);
    }
    Ok(())
}

/// Register a newly authenticated login under its stable session id.
pub async fn register_session(
    user_id: &str,
    sid: &str,
    org_id: Option<&str>,
    authenticated_at_unix: u64,
    refresh_jti: &str,
    ttl_secs: u64,
) -> Result<(), StoreError> {
    let now = now_unix();
    let record = SessionRecord {
        id: sid.to_string(),
        user_id: user_id.to_string(),
        org_id: org_id.map(str::to_string),
        authenticated_at_unix,
        last_seen_at_unix: now,
        expires_at_unix: now.saturating_add(ttl_secs),
        client_ids: Vec::new(),
    };
    let store = use_store();
    store
        .set(
            &refresh_rotation_key(sid),
            &RefreshRotation {
                current_jti: refresh_jti.to_string(),
            },
            Some(ttl_secs),
        )
        .await?;
    if let Err(error) = store_record(&record, ttl_secs).await {
        let _ = store.delete(&refresh_rotation_key(sid)).await;
        return Err(error);
    }
    Ok(())
}

/// Backfill a pre-registry session the first time it participates in a newer
/// flow. Existing per-user expiry is preserved rather than extended.
pub async fn ensure_session_registered(
    user_id: &str,
    sid: &str,
    org_id: Option<&str>,
    authenticated_at_unix: u64,
) -> Result<(), StoreError> {
    let store = use_store();
    if store.hexists(SESSION_DIRECTORY_KEY, sid).await? {
        return Ok(());
    }

    let existing = store
        .hget::<SessionIndexEntry>(&index_key(user_id), sid)
        .await?;
    let expires_at_unix = existing
        .as_ref()
        .map(|entry| entry.expires_at_unix)
        .unwrap_or_else(|| {
            now_unix().saturating_add(jwt_config().refresh_exp.as_seconds_f64().max(1.0) as u64)
        });
    let Some(ttl_secs) = ttl_until(expires_at_unix) else {
        return Ok(());
    };
    let record = SessionRecord {
        id: sid.to_string(),
        user_id: user_id.to_string(),
        org_id: org_id.map(str::to_string),
        authenticated_at_unix,
        last_seen_at_unix: now_unix(),
        expires_at_unix,
        client_ids: Vec::new(),
    };
    store_record(&record, ttl_secs).await
}

/// Atomically consume one native refresh token and install its successor.
///
/// A missing rotation record is initialized once to support sessions created
/// before refresh replay protection was stored separately.
pub async fn rotate_refresh_token(
    sid: &str,
    presented_jti: &str,
    next_jti: &str,
    ttl_secs: u64,
) -> Result<bool, StoreError> {
    let store = use_store();
    let key = refresh_rotation_key(sid);
    let next = RefreshRotation {
        current_jti: next_jti.to_string(),
    };

    let Some(current) = store.get::<RefreshRotation>(&key).await? else {
        return store.set_if_absent(&key, &next, Some(ttl_secs)).await;
    };
    if current.current_jti != presented_jti {
        return Ok(false);
    }

    store
        .compare_and_swap(&key, &current, &next, Some(ttl_secs))
        .await
}

/// Renew session metadata after a refresh token was successfully rotated.
pub async fn renew_session(
    user_id: &str,
    sid: &str,
    org_id: Option<&str>,
    authenticated_at_unix: u64,
    ttl_secs: u64,
) -> Result<(), StoreError> {
    let now = now_unix();
    let mut record = use_store()
        .hget::<SessionRecord>(SESSION_DIRECTORY_KEY, sid)
        .await?
        .unwrap_or_else(|| SessionRecord {
            id: sid.to_string(),
            user_id: user_id.to_string(),
            org_id: org_id.map(str::to_string),
            authenticated_at_unix,
            last_seen_at_unix: now,
            expires_at_unix: now.saturating_add(ttl_secs),
            client_ids: Vec::new(),
        });
    record.org_id = org_id.map(str::to_string);
    record.last_seen_at_unix = now;
    record.expires_at_unix = now.saturating_add(ttl_secs);
    store_record(&record, ttl_secs).await
}

/// Associate an OAuth/OIDC client with an existing login session.
pub async fn register_session_client(
    sid: &str,
    user_id: &str,
    client_id: &str,
) -> Result<bool, StoreError> {
    let store = use_store();
    let Some(mut record) = store
        .hget::<SessionRecord>(SESSION_DIRECTORY_KEY, sid)
        .await?
    else {
        return Ok(false);
    };
    if record.user_id != user_id {
        return Ok(false);
    }
    if record.expires_at_unix <= now_unix() || !store.exists(sid).await? {
        return Ok(false);
    }
    if !record.client_ids.iter().any(|value| value == client_id) {
        record.client_ids.push(client_id.to_string());
        record.client_ids.sort_unstable();
    }
    record.last_seen_at_unix = now_unix();
    let Some(ttl_secs) = ttl_until(record.expires_at_unix) else {
        return Ok(false);
    };
    store
        .hset(SESSION_DIRECTORY_KEY, sid, &record, Some(ttl_secs))
        .await?;
    Ok(true)
}

pub async fn touch_session(sid: &str, user_id: &str) -> Result<bool, StoreError> {
    let store = use_store();
    let Some(mut record) = get_active_session(sid, user_id).await? else {
        return Ok(false);
    };
    record.last_seen_at_unix = now_unix();
    let Some(ttl_secs) = ttl_until(record.expires_at_unix) else {
        return Ok(false);
    };
    store
        .hset(SESSION_DIRECTORY_KEY, sid, &record, Some(ttl_secs))
        .await?;
    Ok(true)
}

/// A user OAuth token remains active only while its backing login session and
/// stable native session record still exists.
pub async fn get_active_session(
    sid: &str,
    user_id: &str,
) -> Result<Option<SessionRecord>, StoreError> {
    let store = use_store();
    let Some(record) = store
        .hget::<SessionRecord>(SESSION_DIRECTORY_KEY, sid)
        .await?
    else {
        return Ok(None);
    };
    if record.user_id != user_id || record.expires_at_unix <= now_unix() {
        return Ok(None);
    }
    if !store.exists(sid).await? {
        return Ok(None);
    }
    Ok(Some(record))
}

pub async fn is_session_active(sid: &str, user_id: &str) -> Result<bool, StoreError> {
    Ok(get_active_session(sid, user_id).await?.is_some())
}

pub async fn get_session(sid: &str) -> Result<Option<SessionRecord>, StoreError> {
    use_store().hget(SESSION_DIRECTORY_KEY, sid).await
}

pub async fn list_sessions() -> Result<Vec<SessionRecord>, StoreError> {
    let store = use_store();
    let sessions = store.hvals::<SessionRecord>(SESSION_DIRECTORY_KEY).await?;
    let now = now_unix();
    let mut active = Vec::with_capacity(sessions.len());
    for session in sessions {
        if session.expires_at_unix > now && store.exists(&session.id).await? {
            active.push(session);
        } else {
            let _ = store.hdel(&index_key(&session.user_id), &session.id).await;
            let _ = store.hdel(SESSION_DIRECTORY_KEY, &session.id).await;
        }
    }
    active.sort_by(|left, right| {
        right
            .last_seen_at_unix
            .cmp(&left.last_seen_at_unix)
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(active)
}

pub async fn session_summary() -> Result<SessionSummary, StoreError> {
    let sessions = list_sessions().await?;
    let active_users = sessions
        .iter()
        .map(|session| session.user_id.as_str())
        .collect::<HashSet<_>>()
        .len();
    Ok(SessionSummary {
        active_sessions: sessions.len(),
        active_users,
    })
}

/// Revoke one stable login session and return the removed record.
pub async fn revoke_session(sid: &str) -> Result<Option<SessionRecord>, StoreError> {
    let store = use_store();
    let Some(record) = get_session(sid).await? else {
        return Ok(None);
    };
    store.delete(&record.id).await?;
    store.delete(&refresh_rotation_key(&record.id)).await?;
    store.hdel(&index_key(&record.user_id), sid).await?;
    store.hdel(SESSION_DIRECTORY_KEY, sid).await?;
    Ok(Some(record))
}

/// Drop a session's registry and refresh-rotation metadata. The subject record
/// is deleted by the caller.
pub async fn forget_session(user_id: &str, sid: &str) {
    let store = use_store();
    if let Err(error) = store.delete(&refresh_rotation_key(sid)).await {
        tracing::warn!(user_id, sid, "failed to remove refresh rotation: {error}");
    }
    if let Err(error) = store.hdel(&index_key(user_id), sid).await {
        tracing::warn!(
            user_id,
            sid,
            "failed to remove session from user index: {error}"
        );
    }
    if let Err(error) = store.hdel(SESSION_DIRECTORY_KEY, sid).await {
        tracing::warn!(
            user_id,
            sid,
            "failed to remove session from directory: {error}"
        );
    }
}

/// Delete every live session of the user. Directory records are authoritative;
/// legacy sid-index entries are also cleaned up during rolling upgrades.
pub async fn revoke_all_sessions(user_id: &str) -> Result<usize, StoreError> {
    let store = use_store();
    let key = index_key(user_id);
    let records = store
        .hgetall::<SessionRecord>(SESSION_DIRECTORY_KEY)
        .await?;
    let mut revoked = 0;
    let mut registered_sids = HashSet::new();
    for (sid, record) in records {
        if record.user_id != user_id {
            continue;
        }
        registered_sids.insert(sid.clone());
        store.delete(&sid).await?;
        store.delete(&refresh_rotation_key(&sid)).await?;
        store.hdel(SESSION_DIRECTORY_KEY, &sid).await?;
        revoked += 1;
    }

    for indexed_id in store.hkeys(&key).await? {
        if !registered_sids.contains(&indexed_id) && store.delete(&indexed_id).await? {
            store.delete(&refresh_rotation_key(&indexed_id)).await?;
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
        if matches && !expired {
            if let Some(record) = get_session(&sid).await? {
                store.delete(&record.id).await?;
                store.delete(&refresh_rotation_key(&record.id)).await?;
                store.hdel(SESSION_DIRECTORY_KEY, &sid).await?;
                revoked += 1;
            } else if store.delete(&sid).await? {
                store.delete(&refresh_rotation_key(&sid)).await?;
                revoked += 1;
            }
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
    let invalid = || ErrorResponse::new(ErrorCode::AuthTokenInvalid);

    let token = token.ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenMissing))?;

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
        return Err(ErrorResponse::new(ErrorCode::AuthUserSessionRequired));
    }

    if claims.sub_id.as_deref() != Some(&subject.id) {
        return Err(invalid());
    }

    ensure_session_registered(
        &subject.id,
        &sid,
        subject.org_id.as_deref(),
        claims.auth_time.unwrap_or(claims.iat) as u64,
    )
    .await
    .map_err(ErrorResponse::internal)?;

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
        .map_err(|_| ErrorResponse::new(ErrorCode::SessionOrganizationMembershipRequired))
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
        register_session(user_id, sid, org, now_unix(), "refresh-jti", 60)
            .await
            .unwrap();
    }

    fn unique(prefix: &str) -> String {
        format!("{prefix}-{}", ulid::Ulid::generate())
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

        // The sid record is the caller's to delete; registry entries are gone.
        assert!(use_store().exists(&sid).await.unwrap());
        assert_eq!(revoke_all_sessions(&user).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn refresh_rotation_preserves_sid_and_rejects_replay() {
        let user = unique("user");
        let sid = unique("sid");
        use_store()
            .set(&sid, &json!({ "id": &user }), Some(60))
            .await
            .unwrap();
        register_session(&user, &sid, None, now_unix(), "jti-1", 60)
            .await
            .unwrap();

        assert!(
            rotate_refresh_token(&sid, "jti-1", "jti-2", 60)
                .await
                .unwrap()
        );
        assert!(
            !rotate_refresh_token(&sid, "jti-1", "jti-3", 60)
                .await
                .unwrap()
        );

        let record = get_session(&sid).await.unwrap().unwrap();
        assert_eq!(record.id, sid);
        assert!(use_store().exists(&record.id).await.unwrap());
    }

    #[tokio::test]
    async fn oauth_client_is_linked_to_active_user_session() {
        let user = unique("user");
        let sid = unique("sid");
        seed_session(&user, &sid, None).await;

        assert!(
            register_session_client(&sid, &user, "beehive")
                .await
                .unwrap()
        );
        let record = get_active_session(&sid, &user).await.unwrap().unwrap();
        assert_eq!(record.client_ids, ["beehive"]);

        revoke_session(&sid).await.unwrap();
    }

    #[tokio::test]
    async fn first_refresh_safely_initializes_legacy_rotation_state() {
        let sid = unique("sid");

        assert!(
            rotate_refresh_token(&sid, "legacy-jti", "jti-1", 60)
                .await
                .unwrap()
        );
        assert!(
            !rotate_refresh_token(&sid, "legacy-jti", "jti-2", 60)
                .await
                .unwrap()
        );
        assert!(
            rotate_refresh_token(&sid, "jti-1", "jti-2", 60)
                .await
                .unwrap()
        );

        use_store()
            .delete(&refresh_rotation_key(&sid))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn summary_counts_sessions_and_distinct_users() {
        let user_a = unique("user");
        let user_b = unique("user");
        let sid_a1 = unique("sid");
        let sid_a2 = unique("sid");
        let sid_b = unique("sid");
        seed_session(&user_a, &sid_a1, None).await;
        seed_session(&user_a, &sid_a2, None).await;
        seed_session(&user_b, &sid_b, None).await;

        let summary = session_summary().await.unwrap();

        assert!(summary.active_sessions >= 3);
        assert!(summary.active_users >= 2);
        revoke_all_sessions(&user_a).await.unwrap();
        revoke_all_sessions(&user_b).await.unwrap();
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
