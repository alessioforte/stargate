use std::sync::atomic::{AtomicBool, Ordering};
use std::{future::Future, sync::Arc};
use tokio::sync::Notify;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    Registration,
    IndexRead,
    ProfileRead,
    SubjectWrite,
    MissingMembership,
}

tokio::task_local! {
    static PAUSE: Arc<Pause>;
}

pub(crate) struct Pause {
    stage: Stage,
    armed: AtomicBool,
    reached: Notify,
    resume: Notify,
}

impl Pause {
    pub(crate) fn new(stage: Stage) -> Arc<Self> {
        Arc::new(Self {
            stage,
            armed: AtomicBool::new(true),
            reached: Notify::new(),
            resume: Notify::new(),
        })
    }

    pub(crate) fn current() -> Option<Arc<Self>> {
        PAUSE.try_with(Arc::clone).ok()
    }

    pub(crate) async fn scope<T>(self: &Arc<Self>, work: impl Future<Output = T>) -> T {
        PAUSE.scope(Arc::clone(self), work).await
    }

    pub(crate) async fn hit(&self, stage: Stage) {
        if stage == self.stage && self.armed.swap(false, Ordering::Relaxed) {
            self.reached.notify_one();
            self.resume.notified().await;
        }
    }

    pub(crate) async fn wait(&self) {
        self.reached.notified().await;
    }

    pub(crate) fn resume(&self) {
        self.resume.notify_one();
    }
}

pub(crate) async fn pause_at(stage: Stage) {
    if let Some(pause) = Pause::current() {
        pause.hit(stage).await;
    }
}

pub(crate) fn inherit_pause<T>(work: impl Future<Output = T>) -> impl Future<Output = T> {
    let pause = Pause::current();
    async move {
        match pause {
            Some(pause) => pause.scope(work).await,
            None => work.await,
        }
    }
}

/// Register a live session of `user_id` with a readable subject.
#[cfg(feature = "memory")]
pub(crate) async fn seed_session(user_id: &str, sid: &str, org_id: Option<&str>) {
    use super::{registry, subjects::PublishedSubject};
    use crate::iam::auth::subject::{Subject, SubjectType};
    use store::Store;

    crate::data::store::use_store()
        .set(
            &store::keys::session::subject(sid),
            &PublishedSubject::from(Subject::new(user_id.into(), SubjectType::User, None)),
            Some(60),
        )
        .await
        .unwrap();
    registry::register_session(
        registry::NewSession {
            user_id,
            sid,
            org_id,
            authenticated_at_unix: registry::now_unix(),
            amr: vec!["pwd".to_string()],
        },
        "refresh-jti",
        60,
    )
    .await
    .unwrap();
}

/// Break a session two ways every path rejects: an expired record, or a
/// subject that no longer decodes.
#[cfg(feature = "memory")]
pub(crate) async fn expire_record(sid: &str) {
    use store::Store;
    let mut record = super::registry::get_session(sid).await.unwrap().unwrap();
    record.expires_at_unix = 1;
    crate::data::store::use_store()
        .set(&store::keys::session::record(sid), &record, Some(60))
        .await
        .unwrap();
}

#[cfg(feature = "memory")]
pub(crate) async fn corrupt_subject(sid: &str) {
    use store::Store;
    crate::data::store::use_store()
        .set(&store::keys::session::subject(sid), &"unreadable", Some(60))
        .await
        .unwrap();
}

#[cfg(feature = "sqlite")]
fn audit_context() -> db::ent::TrustedAuditContext {
    db::ent::TrustedAuditContext::admin_control_plane(
        db::ent::TrustedAdminActor::admin("01JZ000000000000000000000A"),
        db::ent::TrustedAuditRequest::from_http(
            ulid::Ulid::generate().to_string(),
            None,
            None,
            None,
        ),
    )
}

/// A user in the current test database.
#[cfg(feature = "sqlite")]
pub(crate) async fn create_user() -> db::ent::User {
    let name = ulid::Ulid::generate().to_string();
    crate::data::db::create_user(
        db::ent::Profile::new(format!("{name}@example.test"), name),
        db::ent::CredentialType::Password,
        "fixture-password-hash",
        None,
        audit_context(),
    )
    .await
    .unwrap()
}

/// An organization in the current test database with these members.
#[cfg(feature = "sqlite")]
pub(crate) async fn create_organization(members: &[&str]) -> String {
    let org = crate::data::db::create_organization("Sessions", None, None, audit_context())
        .await
        .unwrap();
    for member in members {
        crate::data::db::add_user_to_organization(
            member,
            &org.id,
            "member",
            db::ent::OrganizationMembershipMutation::Create,
            audit_context(),
        )
        .await
        .unwrap();
    }
    org.id
}

/// A fresh session id.
#[cfg(feature = "memory")]
pub(crate) fn new_sid() -> String {
    ulid::Ulid::generate().to_string()
}
