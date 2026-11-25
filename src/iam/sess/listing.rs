//! Paged listing of live sessions, without a key that lists them all.
//!
//! Users come from SQL in descending id order, optionally only the members of
//! one organization; each user's index names their sessions, listed newest
//! first. A page examines a bounded number of users, so its cost does not grow
//! with the sessions of the whole system. Every listed session passes the
//! checks of `load_active_session`: an expired session or one with an
//! unreadable subject is left out as soon as it stops being valid.

use super::registry::{SessionRecord, get_active_session};
use crate::data::store::{use_store, without_security_budget};
use crate::err::{ErrorCode, ErrorResponse};
use db::ent::User;
use futures_util::future::try_join_all;
use store::{Store, StoreError};

/// Users read from SQL in one query.
const USER_BATCH: i64 = 100;
/// Users a page may examine before it returns, full or not.
const USER_BUDGET: usize = 1_000;
/// Store reads in flight at once.
const CONCURRENCY: usize = 32;

/// Which sessions to list. Filters combine.
#[derive(Debug, Default)]
pub struct SessionFilter<'a> {
    pub user_id: Option<&'a str>,
    pub organization_id: Option<&'a str>,
    pub client_id: Option<&'a str>,
}

impl SessionFilter<'_> {
    fn accepts(&self, record: &SessionRecord) -> bool {
        self.organization_id
            .is_none_or(|org_id| record.org_id.as_deref() == Some(org_id))
            && self
                .client_id
                .is_none_or(|client_id| record.client_ids.iter().any(|id| id == client_id))
    }
}

/// Where the next page starts: after session `sid` of `user_id`, or after all
/// of that user's sessions when `sid` is `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCursor {
    pub user_id: String,
    pub sid: Option<String>,
}

impl SessionCursor {
    /// `<user id>.<sid>`; the sid is a ULID and never contains the separator.
    pub fn encode(&self) -> String {
        format!(
            "{}.{}",
            self.user_id,
            self.sid.as_deref().unwrap_or_default()
        )
    }

    pub fn decode(value: &str) -> Option<Self> {
        let (user_id, sid) = value.rsplit_once('.')?;
        (!user_id.is_empty()).then(|| Self {
            user_id: user_id.to_string(),
            sid: (!sid.is_empty()).then(|| sid.to_string()),
        })
    }
}

pub struct ListedSession {
    pub user: User,
    pub record: SessionRecord,
}

pub struct SessionPage {
    pub sessions: Vec<ListedSession>,
    /// `None` once every matching session has been listed.
    pub next: Option<SessionCursor>,
}

/// List up to `limit` live sessions after `after`.
///
/// A page can hold fewer sessions while `next` is set: it stops after
/// examining a bounded number of users, which keeps sparse filters cheap.
pub async fn list_sessions(
    filter: &SessionFilter<'_>,
    after: Option<&SessionCursor>,
    limit: usize,
) -> Result<SessionPage, ErrorResponse> {
    if let (Some(user_id), Some(cursor)) = (filter.user_id, after)
        && cursor.user_id != user_id
    {
        return Err(ErrorResponse::new(ErrorCode::RequestInvalidQuery)
            .with_message("The cursor belongs to another user filter"));
    }
    without_security_budget(async {
        let mut page = Page::new(filter, limit);
        // The rest of the cursor's user comes first.
        if let Some(cursor) = after
            && let Some(sid) = cursor.sid.as_deref()
            && let Some(user) = find_user(&cursor.user_id).await?
            && !page.add(&user, Some(sid)).await?
        {
            return Ok(page.finish());
        }
        if let Some(user_id) = filter.user_id {
            if after.is_none()
                && let Some(user) = find_user(user_id).await?
            {
                page.add(&user, None).await?;
            }
            return Ok(page.finish());
        }

        let mut before = after.map(|cursor| cursor.user_id.clone());
        let mut examined = 0;
        while examined < USER_BUDGET {
            let users = crate::data::db::service()
                .users_before(filter.organization_id, before.as_deref(), USER_BATCH)
                .await
                .map_err(ErrorResponse::internal)?;
            if users.is_empty() {
                return Ok(page.finish());
            }
            examined += users.len();
            let indexes = try_join_all(users.iter().map(|user| indexed_sids(&user.id))).await?;
            for (user, sids) in users.iter().zip(indexes) {
                if !page.add_sids(user, sids, None).await? {
                    return Ok(page.finish());
                }
                before = Some(user.id.clone());
            }
        }
        page.next = before.map(|user_id| SessionCursor { user_id, sid: None });
        Ok(page.finish())
    })
    .await
}

async fn find_user(user_id: &str) -> Result<Option<User>, ErrorResponse> {
    crate::data::db::get_user_by_id(user_id)
        .await
        .map_err(ErrorResponse::internal)
}

async fn indexed_sids(user_id: &str) -> Result<Vec<String>, StoreError> {
    use_store()
        .hkeys(&store::keys::session::index(user_id))
        .await
}

struct Page<'f> {
    filter: &'f SessionFilter<'f>,
    limit: usize,
    sessions: Vec<ListedSession>,
    next: Option<SessionCursor>,
}

impl<'f> Page<'f> {
    fn new(filter: &'f SessionFilter<'f>, limit: usize) -> Self {
        Self {
            filter,
            limit,
            sessions: Vec::with_capacity(limit),
            next: None,
        }
    }

    async fn add(&mut self, user: &User, after_sid: Option<&str>) -> Result<bool, StoreError> {
        let sids = indexed_sids(&user.id).await?;
        self.add_sids(user, sids, after_sid).await
    }

    /// Add the user's live sessions newest first, after `after_sid`. Returns
    /// false once the page is full; `next` then points after its last session.
    async fn add_sids(
        &mut self,
        user: &User,
        mut sids: Vec<String>,
        after_sid: Option<&str>,
    ) -> Result<bool, StoreError> {
        sids.sort_unstable_by(|left, right| right.cmp(left));
        if let Some(after_sid) = after_sid {
            sids.retain(|sid| sid.as_str() < after_sid);
        }
        for chunk in sids.chunks(CONCURRENCY) {
            let sessions =
                try_join_all(chunk.iter().map(|sid| get_active_session(sid, &user.id))).await?;
            for (record, _) in sessions.into_iter().flatten() {
                if !self.filter.accepts(&record) {
                    continue;
                }
                let sid = record.id.clone();
                self.sessions.push(ListedSession {
                    user: user.clone(),
                    record,
                });
                if self.sessions.len() == self.limit {
                    self.next = Some(SessionCursor {
                        user_id: user.id.clone(),
                        sid: Some(sid),
                    });
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    fn finish(self) -> SessionPage {
        SessionPage {
            sessions: self.sessions,
            next: self.next,
        }
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use crate::data::db::with_test_database;
    use crate::iam::sess::registry::register_session_client;
    use crate::iam::sess::test_support::{
        corrupt_subject, create_organization, create_user, expire_record, new_sid, seed_session,
    };

    /// Every page in order, as (user, sid), following `nextCursor`.
    async fn walk(filter: &SessionFilter<'_>, limit: usize) -> Vec<(String, String)> {
        let mut listed = Vec::new();
        let mut cursor = None;
        loop {
            let page = list_sessions(filter, cursor.as_ref(), limit).await.unwrap();
            assert!(page.sessions.len() <= limit);
            listed.extend(
                page.sessions
                    .into_iter()
                    .map(|session| (session.user.id, session.record.id)),
            );
            match page.next {
                Some(next) => cursor = Some(SessionCursor::decode(&next.encode()).unwrap()),
                None => return listed,
            }
        }
    }

    #[tokio::test]
    async fn pages_list_live_sessions_newest_first_and_skip_broken_ones() {
        with_test_database(async {
            let (alice, bob, _idle) = (
                create_user().await,
                create_user().await,
                create_user().await,
            );
            let [a1, a2, a3, a_expired, a_unreadable] = [(); 5].map(|_| new_sid());
            for sid in [&a1, &a2, &a3, &a_expired, &a_unreadable] {
                seed_session(&alice.id, sid, None).await;
            }
            let org = create_organization(&[&alice.id, &bob.id]).await;
            let [b1, b2] = [(); 2].map(|_| new_sid());
            seed_session(&bob.id, &b1, Some(&org)).await;
            seed_session(&bob.id, &b2, None).await;
            expire_record(&a_expired).await;
            corrupt_subject(&a_unreadable).await;
            assert!(
                register_session_client(&b1, &bob.id, "client-1")
                    .await
                    .unwrap()
            );

            // Users by descending id, then each user's sessions by descending sid.
            let mut expected = vec![
                (alice.id.clone(), a1.clone()),
                (alice.id.clone(), a2.clone()),
                (alice.id.clone(), a3.clone()),
                (bob.id.clone(), b1.clone()),
                (bob.id.clone(), b2.clone()),
            ];
            expected.sort_unstable_by(|left, right| right.cmp(left));
            for limit in [1, 2, 100] {
                assert_eq!(walk(&SessionFilter::default(), limit).await, expected);
            }

            let only = |user_id, organization_id, client_id| SessionFilter {
                user_id,
                organization_id,
                client_id,
            };
            assert_eq!(
                walk(&only(Some(&alice.id), None, None), 2).await,
                expected
                    .iter()
                    .filter(|(user, _)| *user == alice.id)
                    .cloned()
                    .collect::<Vec<_>>()
            );
            let b1_only = vec![(bob.id.clone(), b1.clone())];
            assert_eq!(walk(&only(None, Some(&org), None), 1).await, b1_only);
            assert_eq!(walk(&only(None, None, Some("client-1")), 1).await, b1_only);

            let foreign = SessionCursor {
                user_id: bob.id.clone(),
                sid: Some(b2.clone()),
            };
            let error = list_sessions(&only(Some(&alice.id), None, None), Some(&foreign), 2)
                .await
                .err()
                .unwrap();
            assert_eq!(error.code, ErrorCode::RequestInvalidQuery);
        })
        .await;
    }

    #[test]
    fn cursors_round_trip_and_reject_malformed_values() {
        for sid in [Some("01JZ00000000000000000000AA".to_string()), None] {
            let cursor = SessionCursor {
                user_id: "01JZ00000000000000000000U1".into(),
                sid,
            };
            assert_eq!(SessionCursor::decode(&cursor.encode()), Some(cursor));
        }
        for value in ["", ".", "no-separator", ".01JZ00000000000000000000AA"] {
            assert_eq!(SessionCursor::decode(value), None, "{value}");
        }
    }
}
