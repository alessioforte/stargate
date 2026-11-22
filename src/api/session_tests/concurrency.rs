use super::*;
use crate::err::ErrorCode;
use crate::iam::sess::test_support::{Pause, Stage};
use std::time::Duration;

const WIDE: &[&str] = &["read", "write"];

pub(super) async fn set_grants(router: &Router, admin: (&str, &str), user: &User, grants: &[&str]) {
    let path = format!("/admin/users/{}/attrs", user.id);
    json_request(
        router,
        "PUT",
        &path,
        admin,
        json!({"attrs": {"grants": grants}}),
    )
    .await;
}

async fn add_admin_membership(user: &User, org_id: &str) {
    db::add_user_to_organization(
        &user.id,
        org_id,
        "admin",
        OrganizationMembershipMutation::Create,
        context(),
    )
    .await
    .unwrap();
}

async fn registration_races(router: &Router, admin: (&str, &str)) {
    let user = create_user().await;
    set_grants(router, admin, &user, WIDE).await;
    let user = db::get_user_by_id(&user.id).await.unwrap().unwrap();
    let org = db::create_organization("Concurrent registration", None, None, context())
        .await
        .unwrap();
    add_admin_membership(&user, &org.id).await;
    let membership = format!("/admin/users/{}/organizations/{}", user.id, org.id);
    let index = store::keys::session::index(&user.id);

    let pause = Pause::new(Stage::Registration);
    let login = pause.scope(start_session(&user, Some(&org.id)));
    let mutation = async {
        pause.wait().await;
        assert_eq!(use_store().hlen(&index).await.unwrap(), 0);
        set_grants(router, admin, &user, &["read"]).await;
        json_request(router, "PUT", &membership, admin, json!({"role": "member"})).await;
        pause.resume();
    };
    let ((sid, native, _, oauth), ()) = tokio::join!(login, mutation);
    assert_subject(
        [&native, &oauth],
        &json!({"grants": ["read"]}),
        Some((&org.id, "member")),
    )
    .await;
    revise_session(&sid, |record| record.client_ids.clear()).await;
    registry::revoke_session(&sid).await.unwrap();

    let pause = Pause::new(Stage::Registration);
    let login = pause.scope(issuance::start_user_session(
        user.clone(),
        chrono::Utc::now().timestamp() as usize,
        Some(&org.id),
        vec!["pwd".into()],
    ));
    let remove = async {
        pause.wait().await;
        assert_eq!(use_store().hlen(&index).await.unwrap(), 0);
        json_request(router, "DELETE", &membership, admin, Value::Null).await;
        pause.resume();
    };
    let (result, ()) = tokio::join!(login, remove);
    match result {
        Err(error) => assert_eq!(error.code, ErrorCode::SessionOrganizationMembershipRequired),
        Ok(_) => panic!("issued a session for the removed membership"),
    }
    assert_eq!(use_store().hlen(&index).await.unwrap(), 0);
    assert!(
        !registry::list_sessions()
            .await
            .unwrap()
            .iter()
            .any(|session| session.user_id == user.id)
    );
}

async fn readmission_during_revocation(router: &Router, admin: (&str, &str)) {
    let user = create_user().await;
    let org = db::create_organization("Readmission", None, None, context())
        .await
        .unwrap();
    add_admin_membership(&user, &org.id).await;
    let (old_sid, _, _, old_oauth) = start_session(&user, Some(&org.id)).await;
    revise_session(&old_sid, |record| record.client_ids.clear()).await;

    // The SQL deletion has committed. Pause a publisher before the removal
    // handler finishes its session cleanup.
    db::remove_user_from_organization(&user.id, &org.id, context())
        .await
        .unwrap();
    let pause = Pause::new(Stage::MissingMembership);
    let publication = pause.scope(registry::refresh_user_subjects(&user.id));
    let readmit = async {
        pause.wait().await;
        registry::membership_removed(&user.id, &org.id)
            .await
            .unwrap();
        let path = format!("/admin/users/{}/organizations/{}", user.id, org.id);
        json_request(router, "PUT", &path, admin, json!({"role":"member"})).await;
        let session = start_session(&user, Some(&org.id)).await;
        pause.resume();
        session
    };
    let ((), (sid, native, _, oauth)) = tokio::join!(publication, readmit);
    assert_ne!(old_sid, sid);
    assert!(registry::get_session(&old_sid).await.unwrap().is_none());
    userinfo(router, &old_oauth, StatusCode::UNAUTHORIZED).await;
    assert_subject([&native, &oauth], &user.attrs, Some((&org.id, "member"))).await;

    // The publisher must also revoke the observed sid when it is the first
    // to see the missing membership.
    revise_session(&sid, |record| record.client_ids.clear()).await;
    db::remove_user_from_organization(&user.id, &org.id, context())
        .await
        .unwrap();
    registry::refresh_user_subjects(&user.id).await;
    assert!(registry::get_session(&sid).await.unwrap().is_none());
    assert!(
        !use_store()
            .exists(&store::keys::session::subject(&sid))
            .await
            .unwrap()
    );
}

async fn publication_races(router: &Router, admin: (&str, &str)) {
    let user = create_user().await;
    let (sid, native, _, oauth) = start_session(&user, None).await;
    let tokens = [native.as_str(), oauth.as_str()];
    let reduced = json!({"grants": ["read"]});

    // The older mutation commits first and resumes after the reduction has
    // replied. Its publication must read the reduced SQL state.
    let pause = Pause::new(Stage::ProfileRead);
    let older = pause.scope(set_grants(router, admin, &user, WIDE));
    let reduce = async {
        pause.wait().await;
        set_grants(router, admin, &user, &["read"]).await;
        assert_subject(tokens, &reduced, None).await;
        pause.resume();
    };
    tokio::join!(older, reduce);
    assert_subject(tokens, &reduced, None).await;

    // The older writer captured wide SQL grants. The reduction republishes the
    // same grants already in the store, so only its revision fences the late CAS.
    let pause = Pause::new(Stage::SubjectWrite);
    let older = pause.scope(set_grants(router, admin, &user, WIDE));
    let reduce = async {
        pause.wait().await;
        set_grants(router, admin, &user, &["read"]).await;
        assert_subject(tokens, &reduced, None).await;
        pause.resume();
    };
    tokio::join!(older, reduce);
    assert_subject(tokens, &reduced, None).await;

    revise_session(&sid, |record| record.client_ids.clear()).await;
    registry::revoke_session(&sid).await.unwrap();
}

async fn renewed_during_publication(router: &Router, admin: (&str, &str)) {
    let user = create_user().await;
    let (sid, native, refresh, oauth) = start_session(&user, None).await;
    assert!(registry::renew_session(&user.id, &sid, 3).await.unwrap());
    let original_expiry = registry::get_session(&sid)
        .await
        .unwrap()
        .unwrap()
        .expires_at_unix;
    let pause = Pause::new(Stage::IndexRead);
    let publication = pause.scope(set_grants(router, admin, &user, &["read"]));
    let renew = async {
        pause.wait().await;
        let refreshed = json_request(
            router,
            "PUT",
            "/account/refresh-token",
            ("authorization", &bearer(&native)),
            json!({"refresh_token": refresh}),
        )
        .await;
        assert!(
            registry::get_session(&sid)
                .await
                .unwrap()
                .unwrap()
                .expires_at_unix
                > original_expiry
        );
        while chrono::Utc::now().timestamp() < original_expiry as i64 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        pause.resume();
        refreshed
    };
    let ((), refreshed) = tokio::join!(publication, renew);
    assert_subject(
        [refreshed["accessToken"].as_str().unwrap(), &oauth],
        &json!({"grants":["read"]}),
        None,
    )
    .await;
    revise_session(&sid, |record| record.client_ids.clear()).await;
    registry::revoke_session(&sid).await.unwrap();
}

pub(super) async fn run(router: &Router, admin: (&str, &str)) {
    tokio::time::timeout(Duration::from_secs(20), async {
        registration_races(router, admin).await;
        readmission_during_revocation(router, admin).await;
        publication_races(router, admin).await;
        renewed_during_publication(router, admin).await;
        #[cfg(feature = "redis")]
        super::delayed_publication::run(router, admin).await;
    })
    .await
    .expect("session race tests timed out");
}
