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
//!
//! Every function that ends sessions here also notifies the relying parties
//! linked to them (OIDC Back-Channel Logout, see `backchannel_logout`).

use super::backchannel_logout::{self, LogoutDelivery};
use crate::data::store::use_store;
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::auth::jwt::jwt_config;
use crate::etc::auth::subject::{Subject, SubjectType};
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
    pub client_ids: Vec<String>,
    /// How the user authenticated (OIDC `amr`); an email OTP verified in the
    /// session adds `otp` and, after another factor, `mfa`.
    pub amr: Vec<String>,
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

/// A newly authenticated login: who, when, and how (`amr`).
pub struct NewSession<'a> {
    pub user_id: &'a str,
    pub sid: &'a str,
    pub org_id: Option<&'a str>,
    pub authenticated_at_unix: u64,
    pub amr: Vec<String>,
}

/// Register a newly authenticated login under its stable session id.
pub async fn register_session(
    session: NewSession<'_>,
    refresh_jti: &str,
    ttl_secs: u64,
) -> Result<(), StoreError> {
    let now = now_unix();
    let sid = session.sid;
    let record = SessionRecord {
        id: sid.to_string(),
        user_id: session.user_id.to_string(),
        org_id: session.org_id.map(str::to_string),
        authenticated_at_unix: session.authenticated_at_unix,
        last_seen_at_unix: now,
        expires_at_unix: now.saturating_add(ttl_secs),
        client_ids: Vec::new(),
        amr: session.amr,
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

/// Atomically consume one native refresh token and install its successor.
/// Every login stores its first rotation record, so a missing one means the
/// session is gone and the token is rejected.
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
        return Ok(false);
    };
    if current.current_jti != presented_jti {
        return Ok(false);
    }

    store
        .compare_and_swap(&key, &current, &next, Some(ttl_secs))
        .await
}

/// Renew session metadata after a refresh token was successfully rotated.
/// Returns false when the session is no longer registered (e.g. revoked
/// concurrently): it must not be brought back.
pub async fn renew_session(
    user_id: &str,
    sid: &str,
    org_id: Option<&str>,
    ttl_secs: u64,
) -> Result<bool, StoreError> {
    let Some(mut record) = use_store()
        .hget::<SessionRecord>(SESSION_DIRECTORY_KEY, sid)
        .await?
        .filter(|record| record.user_id == user_id)
    else {
        return Ok(false);
    };
    let now = now_unix();
    record.org_id = org_id.map(str::to_string);
    record.last_seen_at_unix = now;
    record.expires_at_unix = now.saturating_add(ttl_secs);
    store_record(&record, ttl_secs).await?;
    Ok(true)
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

/// Record an email OTP the user verified in session `sid` (an MFA step-up):
/// `otp`, and `mfa` when the session already had another factor. Returns
/// false when the session is gone.
pub async fn add_otp_factor(sid: &str, user_id: &str) -> Result<bool, StoreError> {
    let Some(mut record) = get_active_session(sid, user_id).await? else {
        return Ok(false);
    };
    record.amr = oidc::authentication::with_otp(&record.amr);
    let Some(ttl_secs) = ttl_until(record.expires_at_unix) else {
        return Ok(false);
    };
    store_record(&record, ttl_secs).await?;
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
    backchannel_logout::notify(std::slice::from_ref(&record));
    Ok(Some(record))
}

/// End the user's own session `sid` (logout): delete its subject, refresh
/// rotation, and registry entries, whether or not the registry still knows
/// it, and notify its relying parties.
pub async fn end_session(user_id: &str, sid: &str) -> Result<LogoutDelivery, StoreError> {
    let store = use_store();
    let record = get_session(sid)
        .await?
        .filter(|record| record.user_id == user_id);
    store.delete(sid).await?;
    store.delete(&refresh_rotation_key(sid)).await?;
    store.hdel(&index_key(user_id), sid).await?;
    store.hdel(SESSION_DIRECTORY_KEY, sid).await?;
    Ok(backchannel_logout::notify(record.as_slice()))
}

/// Delete every live session of the user (directory records are
/// authoritative) and the user's session index.
pub async fn revoke_all_sessions(user_id: &str) -> Result<usize, StoreError> {
    let store = use_store();
    let records = store
        .hgetall::<SessionRecord>(SESSION_DIRECTORY_KEY)
        .await?;
    let mut revoked = Vec::new();
    for (sid, record) in records {
        if record.user_id != user_id {
            continue;
        }
        store.delete(&sid).await?;
        store.delete(&refresh_rotation_key(&sid)).await?;
        store.hdel(SESSION_DIRECTORY_KEY, &sid).await?;
        revoked.push(record);
    }
    store.delete(&index_key(user_id)).await?;
    backchannel_logout::notify(&revoked);
    Ok(revoked.len())
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
    let mut records = Vec::new();
    for (sid, entry) in entries {
        let expired = entry.expires_at_unix <= now;
        let matches = entry.org_id.as_deref() == Some(org_id);
        if matches && !expired {
            if let Some(record) = get_session(&sid).await? {
                store.delete(&record.id).await?;
                store.delete(&refresh_rotation_key(&record.id)).await?;
                store.hdel(SESSION_DIRECTORY_KEY, &sid).await?;
                revoked += 1;
                records.push(record);
            } else if store.delete(&sid).await? {
                store.delete(&refresh_rotation_key(&sid)).await?;
                revoked += 1;
            }
        }
        if matches || expired {
            let _ = store.hdel(&key, &sid).await;
        }
    }
    backchannel_logout::notify(&records);
    Ok(revoked)
}

// ── Session authentication ───────────────────────────────────────────────────

/// A fully validated user session: access token verified, not revoked, and
/// backed by a live sid record in the store.
pub struct AuthenticatedSession {
    pub user: db::ent::User,
    pub subject: Subject,
    pub sid: String,
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

    let user = crate::data::db::get_user_by_id(&subject.id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(invalid)?;

    Ok(AuthenticatedSession {
        user,
        subject,
        sid,
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
    let memberships = crate::data::db::get_user_organizations(&user.id)
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

    // Oldest membership.
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
        register_session(
            NewSession {
                user_id,
                sid,
                org_id: org,
                authenticated_at_unix: now_unix(),
                amr: vec!["pwd".to_string()],
            },
            "refresh-jti",
            60,
        )
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
    async fn a_verified_email_code_adds_a_factor_to_the_session() {
        let user = unique("user");
        let sid = unique("sid");
        seed_session(&user, &sid, None).await;

        assert!(add_otp_factor(&sid, &user).await.unwrap());
        assert_eq!(
            get_session(&sid).await.unwrap().unwrap().amr,
            vec!["pwd", "otp", "mfa"]
        );
        // Idempotent, and bound to the session's own user.
        assert!(add_otp_factor(&sid, &user).await.unwrap());
        assert_eq!(get_session(&sid).await.unwrap().unwrap().amr.len(), 3);
        assert!(!add_otp_factor(&sid, "someone-else").await.unwrap());
        assert!(!add_otp_factor(&unique("sid"), &user).await.unwrap());
    }

    #[tokio::test]
    async fn end_session_removes_the_session_and_its_registry_entries() {
        let user = unique("user");
        let sid = unique("sid");
        let other = unique("sid");
        seed_session(&user, &sid, None).await;
        seed_session(&user, &other, None).await;

        end_session(&user, &sid).await.unwrap();

        assert!(!use_store().exists(&sid).await.unwrap());
        assert!(get_session(&sid).await.unwrap().is_none());
        assert!(
            !rotate_refresh_token(&sid, "refresh-jti", "next", 60)
                .await
                .unwrap()
        );
        // Other sessions of the user are untouched.
        assert_eq!(revoke_all_sessions(&user).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn end_session_ignores_another_users_registry_record() {
        let owner = unique("user");
        let sid = unique("sid");
        seed_session(&owner, &sid, None).await;

        // A mismatched user still removes the sid but notifies nobody.
        end_session("someone-else", &sid).await.unwrap();

        assert!(!use_store().exists(&sid).await.unwrap());
    }

    #[tokio::test]
    async fn refresh_rotation_preserves_sid_and_rejects_replay() {
        let user = unique("user");
        let sid = unique("sid");
        use_store()
            .set(&sid, &json!({ "id": &user }), Some(60))
            .await
            .unwrap();
        register_session(
            NewSession {
                user_id: &user,
                sid: &sid,
                org_id: None,
                authenticated_at_unix: now_unix(),
                amr: vec!["pwd".to_string()],
            },
            "jti-1",
            60,
        )
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
    async fn refresh_without_rotation_state_is_rejected() {
        let sid = unique("sid");

        assert!(
            !rotate_refresh_token(&sid, "any-jti", "jti-1", 60)
                .await
                .unwrap()
        );
        assert!(
            !use_store()
                .exists(&refresh_rotation_key(&sid))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn renewal_does_not_resurrect_a_revoked_session() {
        let user = unique("user");
        let sid = unique("sid");
        seed_session(&user, &sid, None).await;

        assert!(renew_session(&user, &sid, Some("org_a"), 60).await.unwrap());
        assert_eq!(
            get_session(&sid).await.unwrap().unwrap().org_id.as_deref(),
            Some("org_a")
        );

        revoke_session(&sid).await.unwrap();
        assert!(!renew_session(&user, &sid, None, 60).await.unwrap());
        assert!(get_session(&sid).await.unwrap().is_none());
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

    fn membership(org_id: &str, role: &str, member_since_unix: i64) -> OrgMembership {
        let mut organization = Organization::new(org_id.to_string(), None);
        organization.id = org_id.to_string();
        OrgMembership {
            organization,
            role: role.to_string(),
            member_since: Utc.timestamp_opt(member_since_unix, 0).unwrap(),
        }
    }

    #[test]
    fn requested_org_wins_and_must_be_a_membership() {
        let memberships = vec![
            membership("org_a", "member", 100),
            membership("org_b", "admin", 200),
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
            membership("org_b", "admin", 200),
            membership("org_a", "member", 100),
        ];

        let selected = select_org(&memberships, None, Some("org_gone"))
            .unwrap()
            .unwrap();
        assert_eq!(selected.organization.id, "org_a");
    }

    #[test]
    fn default_org_beats_oldest_membership() {
        let memberships = vec![
            membership("org_a", "member", 100),
            membership("org_b", "admin", 200),
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
}
