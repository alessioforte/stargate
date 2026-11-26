//! Stable login-session registry and org context selection.
//!
//! Each login has one stable standard JWT `sid`. Refresh-token replay
//! protection is tracked separately through a rotating JWT `jti`. A session
//! keeps its subject, refresh rotation and record under its own key tag and
//! changes them in one store transaction: creation, renewal and revocation
//! never stop halfway, and every write after creation requires the state it
//! read, so a revoked session never comes back. The per-user index names the
//! user's sessions for targeted cleanup and listing. Every key and index field
//! expires with the session. No key lists the sessions of every user.
//!
//! The index lives under the user's tag and changes in a separate step. A
//! session is indexed before any token names it, renewal extends the entry
//! before the session, and revocation removes the entry after the session: an
//! entry may outlive its session, but a usable session always has one, so
//! revoking every session of a user always finds it.
//!
//! Deleting a `sid` record kills the session completely: the gateway and
//! account endpoints resolve subjects from the store by sid, and the
//! refresh flow requires the sid record to rotate — a cryptographically
//! valid JWT with no session behind it is rejected everywhere.
//!
//! Every function that ends sessions here also notifies the relying parties
//! linked to them (OIDC Back-Channel Logout, see `backchannel_logout`).

use super::subjects::{self, Publication, PublishedSubject};
use crate::data::store::{commit_confirmed, delete_confirmed, use_store, without_security_budget};
use crate::err::{ErrorCode, ErrorResponse};
use crate::iam::auth::jwt::jwt_config;
use crate::iam::auth::revocation::RevocationConfirmations;
use crate::iam::auth::subject::{Subject, SubjectType};
use crate::iam::oidc::backchannel_logout::{self, LogoutDelivery};
use db::ent::OrgMembership;
use serde::{Deserialize, Serialize};
use store::{Store, StoreError, Transaction};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RefreshRotation {
    current_jti: String,
}

impl RefreshRotation {
    fn new(jti: &str) -> Self {
        Self {
            current_jti: jti.to_string(),
        }
    }
}

fn index_key(user_id: &str) -> String {
    store::keys::session::index(user_id)
}

fn subject_key(sid: &str) -> String {
    store::keys::session::subject(sid)
}

fn refresh_rotation_key(sid: &str) -> String {
    store::keys::session::refresh(sid)
}

fn record_key(sid: &str) -> String {
    store::keys::session::record(sid)
}

pub(super) fn now_unix() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

/// Index the session for its user until `ttl_secs` from now, its expiry.
async fn index(record: &SessionRecord, ttl_secs: u64) -> Result<(), StoreError> {
    let entry = SessionIndexEntry {
        org_id: record.org_id.clone(),
        expires_at_unix: record.expires_at_unix,
    };
    use_store()
        .hset(
            &index_key(&record.user_id),
            &record.id,
            &entry,
            Some(ttl_secs),
        )
        .await
        .map(drop)
}

/// Delete the session's subject, refresh rotation and record together.
async fn delete_state(sid: &str) -> Result<(), StoreError> {
    let delete = Transaction::new()
        .delete(&subject_key(sid))
        .delete(&refresh_rotation_key(sid))
        .delete(&record_key(sid));
    commit_confirmed(&delete).await.map(drop)
}

/// Apply `change` to the live record of `user_id`'s session, keeping its
/// expiry. The write requires the record it changed, so it neither drops a
/// concurrent change nor recreates a revoked session.
async fn update_record(
    sid: &str,
    user_id: &str,
    change: impl Fn(&mut SessionRecord),
) -> Result<bool, StoreError> {
    while let Some((current, _)) = get_active_session(sid, user_id).await? {
        let mut next = current.clone();
        change(&mut next);
        #[cfg(test)]
        super::test_support::pause_at(super::test_support::Stage::RecordWrite).await;
        let update = Transaction::new().require(&subject_key(sid)).swap(
            &record_key(sid),
            &current,
            &next,
        )?;
        if use_store().commit(&update).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A newly authenticated login: who, when, and how (`amr`).
pub struct NewSession<'a> {
    pub sid: &'a str,
    /// The signed-in user, in the organization the session acts in.
    pub subject: Subject,
    pub authenticated_at_unix: u64,
    pub amr: Vec<String>,
}

/// Register a newly authenticated login under its stable session id, then
/// index it. Its tokens must be issued only after this returns.
pub async fn register_session(
    session: NewSession<'_>,
    refresh_jti: &str,
    ttl_secs: u64,
) -> Result<(), StoreError> {
    let now = now_unix();
    let sid = session.sid;
    let record = SessionRecord {
        id: sid.to_string(),
        user_id: session.subject.id.clone(),
        org_id: session.subject.org_id.clone(),
        authenticated_at_unix: session.authenticated_at_unix,
        last_seen_at_unix: now,
        expires_at_unix: now.saturating_add(ttl_secs),
        client_ids: Vec::new(),
        amr: session.amr,
    };
    let create = Transaction::new()
        .set(
            &subject_key(sid),
            &PublishedSubject::from(session.subject),
            ttl_secs,
        )?
        .set(
            &refresh_rotation_key(sid),
            &RefreshRotation::new(refresh_jti),
            ttl_secs,
        )?
        .set(&record_key(sid), &record, ttl_secs)?;
    use_store().commit(&create).await?;
    if let Err(error) = index(&record, ttl_secs).await {
        // No token names the session yet: deleting it only tidies up.
        let _ = delete_state(sid).await;
        return Err(error);
    }
    #[cfg(test)]
    super::test_support::pause_at(super::test_support::Stage::Indexed).await;
    Ok(())
}

/// Consume a native refresh token and extend its session: the next refresh
/// `jti`, the record's times and the subject's lifetime change in one step,
/// so a concurrent revocation either rejects the renewal or deletes the
/// renewed session. A token already consumed, by a replay or a concurrent
/// rotation, ends the session.
pub async fn renew_session(
    user_id: &str,
    sid: &str,
    presented_jti: &str,
    next_jti: &str,
    ttl_secs: u64,
) -> Result<bool, StoreError> {
    let presented = RefreshRotation::new(presented_jti);
    let next = RefreshRotation::new(next_jti);
    let now = now_unix();
    while let Some((current, _)) = get_active_session(sid, user_id).await? {
        let mut renewed = current.clone();
        renewed.last_seen_at_unix = now;
        renewed.expires_at_unix = now.saturating_add(ttl_secs);
        index(&renewed, ttl_secs).await?;
        #[cfg(test)]
        super::test_support::pause_at(super::test_support::Stage::RecordWrite).await;
        let renewal = Transaction::new()
            .require(&subject_key(sid))
            .require_equal(&refresh_rotation_key(sid), &presented)?
            .require_equal(&record_key(sid), &current)?
            .set(&refresh_rotation_key(sid), &next, ttl_secs)?
            .set(&record_key(sid), &renewed, ttl_secs)?
            .expire(&subject_key(sid), ttl_secs);
        if use_store().commit(&renewal).await? {
            return Ok(true);
        }
        let rotation = use_store()
            .get::<RefreshRotation>(&refresh_rotation_key(sid))
            .await?;
        // Unless only the record changed meanwhile, the token was consumed or
        // the session has ended: end it, and the entry extended above.
        if rotation.as_ref() != Some(&presented) {
            end_session(user_id, sid).await?;
            return Ok(false);
        }
    }
    Ok(false)
}

/// Associate an OAuth/OIDC client with an existing login session.
pub async fn register_session_client(
    sid: &str,
    user_id: &str,
    client_id: &str,
) -> Result<bool, StoreError> {
    let now = now_unix();
    update_record(sid, user_id, |record| {
        if !record.client_ids.iter().any(|value| value == client_id) {
            record.client_ids.push(client_id.to_string());
            record.client_ids.sort_unstable();
        }
        record.last_seen_at_unix = now;
    })
    .await
}

/// Record an email OTP the user verified in session `sid` (an MFA step-up):
/// `otp`, and `mfa` when the session already had another factor. Returns
/// false when the session is gone.
pub async fn add_otp_factor(sid: &str, user_id: &str) -> Result<bool, StoreError> {
    update_record(sid, user_id, |record| {
        record.amr = oidc::authentication::with_otp(&record.amr);
    })
    .await
}

pub async fn touch_session(sid: &str, user_id: &str) -> Result<bool, StoreError> {
    let now = now_unix();
    update_record(sid, user_id, |record| record.last_seen_at_unix = now).await
}

/// A live login has a current user subject and a matching, unexpired record.
pub async fn load_active_session(
    sid: &str,
) -> Result<Option<(SessionRecord, Subject)>, StoreError> {
    let (record, subject) = tokio::try_join!(get_session(sid), subjects::read(sid))?;
    Ok(record
        .zip(subject)
        .filter(|(record, subject)| is_active(sid, record, subject, now_unix())))
}

/// The checks every path applies to a session it reads: the record names this
/// sid and has not expired, and the subject is the record's user.
fn is_active(sid: &str, record: &SessionRecord, subject: &Subject, now: u64) -> bool {
    record.id == sid
        && record.expires_at_unix > now
        && subject.sub_type == SubjectType::User
        && subject.id == record.user_id
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

/// The session's record. An unreadable record cannot describe a live
/// session, so it reads as missing, like an unreadable subject; transport
/// failures still reach the caller.
pub async fn get_session(sid: &str) -> Result<Option<SessionRecord>, StoreError> {
    match use_store().get(&record_key(sid)).await {
        Err(StoreError::DeserializationFailed(_)) => Ok(None),
        result => result,
    }
}

/// The session's record, and whether revoking it counts: a readable record
/// or a subject remains.
async fn revocable(sid: &str) -> Result<(Option<SessionRecord>, bool), StoreError> {
    let subject = subject_key(sid);
    let (record, subject) = tokio::try_join!(get_session(sid), use_store().exists(&subject))?;
    let counted = subject || record.is_some();
    Ok((record, counted))
}

/// Revoke one stable login session, even when its record is missing or
/// unreadable, and return the removed record.
pub async fn revoke_session(sid: &str) -> Result<Option<SessionRecord>, StoreError> {
    let record = get_session(sid).await?;
    let mut confirmations = RevocationConfirmations::default();
    confirmations.record(delete_state(sid).await)?;
    if let Some(record) = &record {
        use_store().hdel(&index_key(&record.user_id), sid).await?;
        backchannel_logout::notify(std::slice::from_ref(record));
    }
    confirmations.finish(record)
}

/// End a specific login session for this user: delete its subject, refresh
/// rotation, record and index entry, whether or not the registry still knows
/// it, and notify its relying parties.
pub async fn end_session(user_id: &str, sid: &str) -> Result<LogoutDelivery, StoreError> {
    let record = get_session(sid)
        .await?
        .filter(|record| record.user_id == user_id);
    let mut confirmations = RevocationConfirmations::default();
    confirmations.record(delete_state(sid).await)?;
    use_store().hdel(&index_key(user_id), sid).await?;
    confirmations.finish(backchannel_logout::notify(record.as_slice()))
}

/// End the session a token names: its owner's when the token carries one.
pub async fn end_token_session(claims: &jwt::Claims) -> Result<(), StoreError> {
    let Some(sid) = claims.sid.as_deref() else {
        return Ok(());
    };
    match claims.sub_id.as_deref() {
        Some(user_id) => end_session(user_id, sid).await.map(drop),
        None => revoke_session(sid).await.map(drop),
    }
}

/// Delete the user's indexed sessions and notify linked clients when their
/// records are readable. A session registered meanwhile keeps its index entry,
/// because only the observed fields are removed.
pub async fn revoke_all_sessions(user_id: &str) -> Result<usize, StoreError> {
    without_security_budget(async {
        let store = use_store();
        let key = index_key(user_id);
        let mut revoked = 0;
        let mut records = Vec::new();
        let mut confirmations = RevocationConfirmations::default();
        for sid in store.hkeys(&key).await? {
            let (record, counted) = revocable(&sid).await?;
            if record
                .as_ref()
                .is_some_and(|record| record.user_id != user_id)
            {
                store.hdel(&key, &sid).await?;
                continue;
            }
            confirmations.record(delete_state(&sid).await)?;
            revoked += usize::from(counted);
            records.extend(record);
            store.hdel(&key, &sid).await?;
        }
        confirmations
            .record(delete_confirmed(&store::keys::account::login_known(user_id)).await)?;
        backchannel_logout::notify(&records);
        confirmations.finish(revoked)
    })
    .await
}

/// Delete the user's sessions whose active org is `org_id`; org-less and
/// other-org sessions survive. Lazily prunes expired index entries. Returns
/// the number of sessions actually deleted.
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
                let (record, counted) = revocable(&sid).await?;
                confirmations.record(delete_state(&sid).await)?;
                revoked += usize::from(counted);
                records.extend(record);
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
    use super::super::test_support::{Pause, Stage, corrupt_subject, expire_record, seed_session};
    use super::*;

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

    #[tokio::test]
    async fn revocation_continues_past_unreadable_records() {
        let user = unique("user");
        let unreadable = unique("sid");
        let valid = unique("sid");
        let alone = unique("sid");
        for sid in [&unreadable, &valid, &alone] {
            seed_session(&user, sid, None).await;
        }
        for sid in [&unreadable, &alone] {
            use_store()
                .set(&record_key(sid), &"unreadable record", Some(60))
                .await
                .unwrap();
        }

        assert_eq!(revoke_session(&alone).await.unwrap(), None);
        assert!(
            !use_store()
                .exists(&store::keys::session::subject(&alone))
                .await
                .unwrap()
        );
        // Without a readable record the owner is unknown: the index entry
        // stays until the user's revocation or its own expiry.
        assert!(
            use_store()
                .hexists(&index_key(&user), &alone)
                .await
                .unwrap()
        );
        assert_eq!(revoke_all_sessions(&user).await.unwrap(), 2);
        for sid in [&unreadable, &valid] {
            for key in [store::keys::session::subject(sid), record_key(sid)] {
                assert!(!use_store().exists(&key).await.unwrap(), "{key}");
            }
        }
        assert!(
            use_store()
                .hkeys(&index_key(&user))
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn active_sessions_need_a_readable_subject_of_their_owner() {
        let user = unique("user");
        let [valid, invalid, expired, wrong_owner] = ["", "", "", ""].map(|_| unique("sid"));
        for sid in [&valid, &invalid, &expired, &wrong_owner] {
            seed_session(&user, sid, None).await;
        }
        expire_record(&expired).await;
        corrupt_subject(&invalid).await;
        use_store()
            .set(
                &store::keys::session::subject(&wrong_owner),
                &subjects::PublishedSubject::from(Subject::new(
                    unique("other"),
                    SubjectType::User,
                    None,
                )),
                Some(60),
            )
            .await
            .unwrap();

        assert!(load_active_session(&valid).await.unwrap().is_some());
        for sid in [&invalid, &expired, &wrong_owner] {
            assert!(load_active_session(sid).await.unwrap().is_none(), "{sid}");
        }
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
            !renew_session(&user, &sid, "refresh-jti", "next", 60)
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
        seed_session(&user, &sid, None).await;

        assert!(
            renew_session(&user, &sid, "refresh-jti", "jti-2", 60)
                .await
                .unwrap()
        );
        assert!(
            !renew_session(&user, &sid, "refresh-jti", "jti-3", 60)
                .await
                .unwrap()
        );

        assert_session_gone(&user, &sid).await;
        assert!(
            !renew_session(&user, &sid, "jti-2", "jti-4", 60)
                .await
                .unwrap()
        );
    }

    async fn assert_session_gone(user: &str, sid: &str) {
        for key in [subject_key(sid), refresh_rotation_key(sid), record_key(sid)] {
            assert!(!use_store().exists(&key).await.unwrap(), "{key}");
        }
        assert!(!use_store().hexists(&index_key(user), sid).await.unwrap());
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
        let user = unique("user");
        let sid = unique("sid");

        assert!(
            !renew_session(&user, &sid, "any-jti", "jti-1", 60)
                .await
                .unwrap()
        );
        assert_session_gone(&user, &sid).await;
    }

    #[tokio::test]
    async fn renewal_does_not_resurrect_a_revoked_session() {
        let user = unique("user");
        let sid = unique("sid");
        seed_session(&user, &sid, Some("org_a")).await;

        assert!(
            renew_session(&user, &sid, "refresh-jti", "jti-2", 60)
                .await
                .unwrap()
        );
        assert_eq!(
            get_session(&sid).await.unwrap().unwrap().org_id.as_deref(),
            Some("org_a")
        );

        revoke_session(&sid).await.unwrap();
        assert!(
            !renew_session(&user, &sid, "jti-2", "jti-3", 60)
                .await
                .unwrap()
        );
        assert_session_gone(&user, &sid).await;
    }

    #[tokio::test]
    async fn record_updates_keep_concurrent_changes() {
        let user = unique("user");
        let sid = unique("sid");
        seed_session(&user, &sid, None).await;

        let pause = Pause::new(Stage::RecordWrite);
        let touch = pause.scope(touch_session(&sid, &user));
        let link = async {
            pause.wait().await;
            assert!(
                register_session_client(&sid, &user, "client")
                    .await
                    .unwrap()
            );
            pause.resume();
        };
        let (touched, ()) = tokio::join!(touch, link);
        assert!(touched.unwrap());
        assert_eq!(
            get_session(&sid).await.unwrap().unwrap().client_ids,
            ["client"]
        );

        let pause = Pause::new(Stage::RecordWrite);
        let touch = pause.scope(touch_session(&sid, &user));
        let revoke = async {
            pause.wait().await;
            end_session(&user, &sid).await.unwrap();
            pause.resume();
        };
        let (touched, ()) = tokio::join!(touch, revoke);
        assert!(!touched.unwrap());
        assert_session_gone(&user, &sid).await;
    }

    #[tokio::test]
    async fn a_renewal_keeps_the_entry_of_a_live_session() {
        let user = unique("user");
        let sid = unique("sid");
        seed_session(&user, &sid, None).await;

        // A change to the record between the renewal's read and its write
        // retries the renewal instead of ending the session.
        let pause = Pause::new(Stage::RecordWrite);
        let renew = pause.scope(renew_session(&user, &sid, "refresh-jti", "jti-2", 120));
        let link = async {
            pause.wait().await;
            assert!(
                register_session_client(&sid, &user, "client")
                    .await
                    .unwrap()
            );
            pause.resume();
        };
        let (renewed, ()) = tokio::join!(renew, link);
        assert!(renewed.unwrap());
        let record = get_session(&sid).await.unwrap().unwrap();
        assert_eq!(record.client_ids, ["client"]);
        let entry = use_store()
            .hget::<SessionIndexEntry>(&index_key(&user), &sid)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.expires_at_unix, record.expires_at_unix);
        revoke_all_sessions(&user).await.unwrap();
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
