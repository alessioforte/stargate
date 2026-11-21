use crate::data::store::use_store;
use crate::iam::auth::subject::{Subject, SubjectType};
use db::service::Service;
use serde::{Deserialize, Serialize};
use store::{Store, StoreError};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PublishedSubject {
    pub(crate) subject: Subject,
    pub(crate) revision: u64,
}

impl From<Subject> for PublishedSubject {
    fn from(subject: Subject) -> Self {
        Self {
            subject,
            revision: 0,
        }
    }
}

pub(crate) async fn read(sid: &str) -> Result<Option<Subject>, StoreError> {
    match use_store()
        .get::<PublishedSubject>(&store::keys::session::subject(sid))
        .await
    {
        Ok(value) => Ok(value.map(|published| published.subject)),
        // An unreadable subject cannot authorize a session. Transport failures
        // still reach callers as store errors.
        Err(StoreError::DeserializationFailed(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

#[derive(Debug)]
pub(crate) enum Publication {
    Updated,
    Gone,
    MembershipRemoved,
}

/// Sample the store before SQL so even a write delivered after client timeout
/// cannot replace a more recent publication on another connection.
pub(crate) async fn publish(
    store: &impl Store,
    db: &Service,
    user_id: &str,
    sid: &str,
    org_id: Option<&str>,
) -> anyhow::Result<Publication> {
    let key = store::keys::session::subject(sid);
    while let Some(current) = store.get::<PublishedSubject>(&key).await? {
        #[cfg(test)]
        super::test_support::pause_at(super::test_support::Stage::ProfileRead).await;
        let Some((attrs, role)) = db.session_profile(user_id, org_id).await? else {
            return Ok(Publication::Gone);
        };
        if org_id.is_some() && role.is_none() {
            return Ok(Publication::MembershipRemoved);
        }
        let mut subject = Subject::new(user_id.to_owned(), SubjectType::User, Some(attrs));
        subject.org_id = org_id.map(str::to_owned);
        subject.org_role = role;
        let next = PublishedSubject {
            subject,
            revision: current
                .revision
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Session subject revision exhausted"))?,
        };
        #[cfg(test)]
        super::test_support::pause_at(super::test_support::Stage::SubjectWrite).await;
        if store.replace_if_equal(&key, &current, &next).await? {
            return Ok(Publication::Updated);
        }
    }
    Ok(Publication::Gone)
}
