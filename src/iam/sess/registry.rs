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

use super::subjects::{self, Publication};
use crate::data::store::{delete_confirmed, use_store, without_security_budget};
use crate::err::{ErrorCode, ErrorResponse};
use crate::iam::auth::jwt::jwt_config;
use crate::iam::auth::revocation::RevocationConfirmations;
use crate::iam::auth::subject::{Subject, SubjectType};
use crate::iam::oidc::backchannel_logout::{self, LogoutDelivery};
use db::ent::OrgMembership;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use store::{Store, StoreError};

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
    store::keys::session::index(user_id)
}

fn refresh_rotation_key(sid: &str) -> String {
    store::keys::session::refresh(sid)
}

fn directory_key() -> String {
    store::keys::session::directory()
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
        .hset(&directory_key(), &record.id, record, Some(ttl_secs))
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
        revoke_session(sid).await?;
        return Ok(false);
    }

    let rotated = store
        .compare_and_swap(&key, &current, &next, Some(ttl_secs))
        .await?;
    // A competing rotation consumed this token too. Treat concurrent reuse the
    // same as a replay observed before the CAS.
    if !rotated {
        revoke_session(sid).await?;
    }
    Ok(rotated)
}

/// Renew a session after native refresh rotation. Extend the existing
/// subject's TTL so a concurrent profile or membership write keeps its value.
pub async fn renew_session(user_id: &str, sid: &str, ttl_secs: u64) -> Result<bool, StoreError> {
    let Some(mut record) = use_store()
        .hget::<SessionRecord>(&directory_key(), sid)
        .await?
        .filter(|record| record.user_id == user_id)
    else {
        return Ok(false);
    };
    if !use_store()
        .expire(&store::keys::session::subject(sid), ttl_secs)
        .await?
    {
        return Ok(false);
    }
    let now = now_unix();
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
    let Some((mut record, _)) = get_active_session(sid, user_id).await? else {
        return Ok(false);
    };
    if !record.client_ids.iter().any(|value| value == client_id) {
        record.client_ids.push(client_id.to_string());
        record.client_ids.sort_unstable();
    }
    record.last_seen_at_unix = now_unix();
    let Some(ttl_secs) = ttl_until(record.expires_at_unix) else {
        return Ok(false);
    };
    store
        .hset(&directory_key(), sid, &record, Some(ttl_secs))
        .await?;
    Ok(true)
}

/// Record an email OTP the user verified in session `sid` (an MFA step-up):
/// `otp`, and `mfa` when the session already had another factor. Returns
/// false when the session is gone.
pub async fn add_otp_factor(sid: &str, user_id: &str) -> Result<bool, StoreError> {
    let Some((mut record, _)) = get_active_session(sid, user_id).await? else {
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
    let Some((mut record, _)) = get_active_session(sid, user_id).await? else {
        return Ok(false);
    };
    record.last_seen_at_unix = now_unix();
    let Some(ttl_secs) = ttl_until(record.expires_at_unix) else {
        return Ok(false);
    };
    store
        .hset(&directory_key(), sid, &record, Some(ttl_secs))
        .await?;
    Ok(true)
}

/// A live login has a current user subject and a matching directory record.
pub async fn load_active_session(
    sid: &str,
) -> Result<Option<(SessionRecord, Subject)>, StoreError> {
    let Some(record) = get_session(sid).await? else {
        return Ok(None);
    };
    let Some(subject) = subjects::read(sid).await? else {
        return Ok(None);
    };
    if record.id != sid
        || record.expires_at_unix <= now_unix()
        || subject.sub_type != SubjectType::User
        || subject.id != record.user_id
    {
        return Ok(None);
    }
    Ok(Some((record, subject)))
}

pub async fn get_active_session(
    sid: &str,
    user_id: &str,
) -> Result<Option<(SessionRecord, Subject)>, StoreError> {
    Ok(load_active_session(sid)
        .await?
        .filter(|(record, _)| record.user_id == user_id))
}

/// Resolve a verified user token to a live session owned by its subject.
pub async fn active_token_session(
    claims: &jwt::Claims,
) -> Result<Option<(SessionRecord, Subject)>, StoreError> {
    let (Some(sid), Some(user_id)) = (claims.sid.as_deref(), claims.sub_id.as_deref()) else {
        return Ok(None);
    };
    if sid.is_empty()
        || user_id.is_empty()
        || (claims.azp.is_some() && (claims.auth_time.is_none() || claims.sub != user_id))
    {
        return Ok(None);
    }
    get_active_session(sid, user_id).await
}

pub async fn get_session(sid: &str) -> Result<Option<SessionRecord>, StoreError> {
    use_store().hget(&directory_key(), sid).await
}

pub async fn list_sessions() -> Result<Vec<SessionRecord>, StoreError> {
    without_security_budget(async {
        let store = use_store();
        let sessions = store.hvals::<SessionRecord>(&directory_key()).await?;
        let mut active = Vec::with_capacity(sessions.len());
        for session in sessions {
            if let Some((record, _)) = load_active_session(&session.id).await? {
                active.push(record);
            }
        }
        active.sort_by(|left, right| {
            right
                .last_seen_at_unix
                .cmp(&left.last_seen_at_unix)
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(active)
    })
    .await
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
    let mut confirmations = RevocationConfirmations::default();
    confirmations.record(delete_confirmed(&store::keys::session::subject(&record.id)).await)?;
    store.delete(&refresh_rotation_key(&record.id)).await?;
    store.hdel(&index_key(&record.user_id), sid).await?;
    store.hdel(&directory_key(), sid).await?;
    backchannel_logout::notify(std::slice::from_ref(&record));
    confirmations.finish(Some(record))
}

/// End a specific login session for this user: delete its subject, refresh
/// rotation, and registry entries, whether or not the registry still knows
/// it, and notify its relying parties.
pub async fn end_session(user_id: &str, sid: &str) -> Result<LogoutDelivery, StoreError> {
    let store = use_store();
    let record = get_session(sid)
        .await?
        .filter(|record| record.user_id == user_id);
    let mut confirmations = RevocationConfirmations::default();
    confirmations.record(delete_confirmed(&store::keys::session::subject(sid)).await)?;
    store.delete(&refresh_rotation_key(sid)).await?;
    store.hdel(&index_key(user_id), sid).await?;
    store.hdel(&directory_key(), sid).await?;
    confirmations.finish(backchannel_logout::notify(record.as_slice()))
}

/// Delete the user's indexed sessions and notify linked clients when a
/// matching directory record is available.
pub async fn revoke_all_sessions(user_id: &str) -> Result<usize, StoreError> {
    without_security_budget(async {
        let store = use_store();
        let key = index_key(user_id);
        let entries = store.hgetall::<SessionIndexEntry>(&key).await?;
        let mut revoked = Vec::new();
        let mut confirmations = RevocationConfirmations::default();
        for sid in entries.keys() {
            let record = get_session(sid).await?;
            if record
                .as_ref()
                .is_some_and(|record| record.user_id != user_id)
            {
                store.hdel(&key, sid).await?;
                continue;
            }
            confirmations.record(delete_confirmed(&store::keys::session::subject(sid)).await)?;
            store.delete(&refresh_rotation_key(sid)).await?;
            if let Some(record) = record {
                store.hdel(&directory_key(), sid).await?;
                revoked.push(record);
            }
            store.hdel(&key, sid).await?;
        }
        confirmations
            .record(delete_confirmed(&store::keys::account::login_known(user_id)).await)?;
        backchannel_logout::notify(&revoked);
        confirmations.finish(revoked.len())
    })
    .await
}

/// Delete the user's sessions whose active org is `org_id`; org-less and
/// other-org sessions survive. Lazily prunes expired index entries. Returns
/// the number of session records actually deleted.
pub async fn revoke_org_sessions(user_id: &str, org_id: &str) -> Result<usize, StoreError> {
    without_security_budget(async {
        let store = use_store();
        let key = index_key(user_id);
        let entries = store.hgetall::<SessionIndexEntry>(&key).await?;
        let now = now_unix();
        let mut revoked = 0;
        let mut records = Vec::new();
        let mut confirmations = RevocationConfirmations::default();
        for (sid, entry) in entries {
            let expired = entry.expires_at_unix <= now;
            let matches = entry.org_id.as_deref() == Some(org_id);
            if matches && !expired {
                if let Some(record) = get_session(&sid).await? {
                    confirmations.record(
                        delete_confirmed(&store::keys::session::subject(&record.id)).await,
                    )?;
                    store.delete(&refresh_rotation_key(&record.id)).await?;
                    store.hdel(&directory_key(), &sid).await?;
                    revoked += 1;
                    records.push(record);
                } else if confirmations
                    .record(delete_confirmed(&store::keys::session::subject(&sid)).await)?
                    .unwrap_or(true)
                {
                    // A shortfall hides the deletion count. Clean up refresh state anyway.
                    store.delete(&refresh_rotation_key(&sid)).await?;
                    revoked += 1;
                }
            }
            if matches || expired {
                store.hdel(&key, &sid).await?;
            }
        }
        backchannel_logout::notify(&records);
        confirmations.finish(revoked)
    })
    .await
}

/// Reconcile a session after registration and before returning its tokens.
pub(super) async fn reconcile_new_session(
    user_id: &str,
    sid: &str,
    org_id: Option<&str>,
) -> Result<(), ErrorResponse> {
    match subjects::publish(
        use_store(),
        &crate::data::db::service(),
        user_id,
        sid,
        org_id,
    )
    .await
    .map_err(|error| match error.downcast::<StoreError>() {
        Ok(error) => ErrorResponse::from(error),
        Err(error) => ErrorResponse::internal(error),
    })? {
        Publication::Updated => Ok(()),
        Publication::Gone => Err(ErrorResponse::new(ErrorCode::AuthTokenInvalid)),
        Publication::MembershipRemoved => Err(ErrorResponse::new(
            ErrorCode::SessionOrganizationMembershipRequired,
        )),
    }
}

/// Finish updating the user's live sessions even if the HTTP caller disconnects
/// after its SQL mutation committed.
pub async fn refresh_user_subjects(user_id: &str) {
    let db = crate::data::db::service();
    let id = user_id.to_owned();
    let work = async move {
        if let Err(error) = refresh_subjects_from_db(&db, &id).await {
            tracing::error!(user_id = id, %error, "Failed to refresh user session subjects");
        }
    };
    #[cfg(test)]
    let work = super::test_support::inherit_pause(work);
    if let Err(error) = tokio::spawn(work).await {
        tracing::error!(user_id, %error, "Session publication worker failed");
    }
}

async fn refresh_subjects_from_db(db: &db::service::Service, user_id: &str) -> anyhow::Result<()> {
    let entries = use_store()
        .hgetall::<SessionIndexEntry>(&index_key(user_id))
        .await?;
    #[cfg(test)]
    super::test_support::pause_at(super::test_support::Stage::IndexRead).await;
    for (sid, entry) in entries {
        match subjects::publish(use_store(), db, user_id, &sid, entry.org_id.as_deref()).await {
            Ok(Publication::MembershipRemoved) => {
                #[cfg(test)]
                super::test_support::pause_at(super::test_support::Stage::MissingMembership).await;
                if let Err(error) = end_session(user_id, &sid).await {
                    tracing::error!(user_id, sid, %error, "Failed to revoke session after membership removal");
                }
            }
            Ok(Publication::Updated | Publication::Gone) => {}
            Err(error) => {
                tracing::error!(user_id, sid, %error, "Failed to refresh user session subject")
            }
        }
    }
    Ok(())
}

pub async fn membership_removed(user_id: &str, org_id: &str) -> Result<(), StoreError> {
    let revocation = revoke_org_sessions(user_id, org_id).await;
    refresh_user_subjects(user_id).await;
    revocation.map(|_| ())
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

    if crate::iam::auth::revocation::is_revoked(&claims).await? {
        return Err(invalid());
    }

    let sid = claims.sid.clone().ok_or_else(invalid)?;
    let subject = crate::iam::sess::subjects::read(&sid)
        .await?
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
            .set(
                &store::keys::session::subject(sid),
                &subjects::PublishedSubject::from(Subject::new(
                    user_id.into(),
                    SubjectType::User,
                    None,
                )),
                Some(60),
            )
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
        assert!(
            !use_store()
                .exists(&store::keys::session::subject(&sid_a))
                .await
                .unwrap()
        );
        assert!(
            !use_store()
                .exists(&store::keys::session::subject(&sid_b))
                .await
                .unwrap()
        );
        // Idempotent: nothing left to revoke.
        assert_eq!(revoke_all_sessions(&user).await.unwrap(), 0);
    }

    #[test]
    fn revoke_all_does_not_read_other_users_directory_records() {
        const CHILD: &str = "STARGATE_REVOKE_INDEX_TEST";
        if std::env::var_os(CHILD).is_none() {
            let output = crate::conf::child_test::run(
                "iam::sess::registry::index_tests::revoke_all_does_not_read_other_users_directory_records",
                |command| {
                    command.env(CHILD, "1");
                },
            );
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let user = unique("user");
            let sid = unique("sid");
            seed_session(&user, &sid, None).await;
            let foreign = unique("sid");
            use_store()
                .hset(&directory_key(), &foreign, &"unreadable record", Some(60))
                .await
                .unwrap();
            assert_eq!(revoke_all_sessions(&user).await.unwrap(), 1);
            assert!(
                use_store()
                    .hexists(&directory_key(), &foreign)
                    .await
                    .unwrap()
            );
            assert!(
                !use_store()
                    .exists(&store::keys::session::subject(&sid))
                    .await
                    .unwrap()
            );
        });
    }

    #[tokio::test]
    async fn list_excludes_unreadable_subjects() {
        let user = unique("user");
        let valid = unique("sid");
        let invalid = unique("sid");
        let expired = unique("sid");
        let wrong_owner = unique("sid");
        seed_session(&user, &valid, None).await;
        seed_session(&user, &invalid, None).await;
        seed_session(&user, &expired, None).await;
        seed_session(&user, &wrong_owner, None).await;
        let mut record = get_session(&expired).await.unwrap().unwrap();
        record.expires_at_unix = 0;
        use_store()
            .hset(&directory_key(), &expired, &record, Some(60))
            .await
            .unwrap();
        let other = Subject::new(unique("other"), SubjectType::User, None);
        use_store()
            .set(
                &store::keys::session::subject(&wrong_owner),
                &subjects::PublishedSubject::from(other),
                Some(60),
            )
            .await
            .unwrap();
        use_store()
            .set(
                &store::keys::session::subject(&invalid),
                &"unreadable",
                Some(60),
            )
            .await
            .unwrap();

        let sessions = list_sessions().await.unwrap();
        assert!(sessions.iter().any(|record| record.id == valid));
        assert!(!sessions.iter().any(|record| record.id == invalid));
        assert!(!sessions.iter().any(|record| record.id == expired));
        assert!(!sessions.iter().any(|record| record.id == wrong_owner));
        revoke_all_sessions(&user).await.unwrap();
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
        assert!(
            !use_store()
                .exists(&store::keys::session::subject(&sid_target))
                .await
                .unwrap()
        );
        assert!(
            use_store()
                .exists(&store::keys::session::subject(&sid_other_org))
                .await
                .unwrap()
        );
        assert!(
            use_store()
                .exists(&store::keys::session::subject(&sid_orgless))
                .await
                .unwrap()
        );
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

        assert!(
            !use_store()
                .exists(&store::keys::session::subject(&sid))
                .await
                .unwrap()
        );
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

        assert!(
            !use_store()
                .exists(&store::keys::session::subject(&sid))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn refresh_rotation_preserves_sid_and_rejects_replay() {
        let user = unique("user");
        let sid = unique("sid");
        use_store()
            .set(
                &store::keys::session::subject(&sid),
                &json!({ "id": &user }),
                Some(60),
            )
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

        assert!(get_session(&sid).await.unwrap().is_none());
        assert!(
            !use_store()
                .exists(&store::keys::session::subject(&sid))
                .await
                .unwrap()
        );
        assert!(
            !rotate_refresh_token(&sid, "jti-2", "jti-4", 60)
                .await
                .unwrap()
        );
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
        let (record, _) = get_active_session(&sid, &user).await.unwrap().unwrap();
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
        seed_session(&user, &sid, Some("org_a")).await;

        assert!(renew_session(&user, &sid, 60).await.unwrap());
        assert_eq!(
            get_session(&sid).await.unwrap().unwrap().org_id.as_deref(),
            Some("org_a")
        );

        revoke_session(&sid).await.unwrap();
        assert!(!renew_session(&user, &sid, 60).await.unwrap());
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
