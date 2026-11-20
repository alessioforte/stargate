use super::*;
use crate::err::ErrorCode;
use crate::iam::sess::test_support::{Pause, Stage};
use std::time::Duration;

const WIDE: &[&str] = &["read", "write"];

async fn set_grants(router: &Router, admin: (&str, &str), user: &User, grants: &[&str]) {
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

async fn publication_races(router: &Router, admin: (&str, &str)) {
    let user = create_user().await;
    let (sid, native, _, oauth) = start_session(&user, None).await;
    let tokens = [native.as_str(), oauth.as_str()];
    let reduced = json!({"grants": ["read"]});

    // The older mutation commits first and resumes after the reduction has
    // replied. Its publication must read the reduced SQL state.
    let pause = Pause::new(Stage::SubjectLock);
    let older = pause.scope(set_grants(router, admin, &user, WIDE));
    let reduce = async {
        pause.wait().await;
        set_grants(router, admin, &user, &["read"]).await;
        assert_subject(tokens, &reduced, None).await;
        pause.resume();
    };
    tokio::join!(older, reduce);
    assert_subject(tokens, &reduced, None).await;

    // An older publication already holding its snapshot has to finish before
    // the reduction can respond. Keep it parked while polling that request.
    let pause = Pause::new(Stage::SubjectWrite);
    let older = pause.scope(set_grants(router, admin, &user, WIDE));
    let reduce = async {
        pause.wait().await;
        let reduction = set_grants(router, admin, &user, &["read"]);
        tokio::pin!(reduction);
        assert!(
            tokio::time::timeout(Duration::from_millis(250), &mut reduction)
                .await
                .is_err(),
            "reduction replied while an older SQL snapshot could still be published"
        );
        pause.resume();
        reduction.await;
        assert_subject(tokens, &reduced, None).await;
    };
    tokio::join!(older, reduce);
    assert_subject(tokens, &reduced, None).await;

    revise_session(&sid, |record| record.client_ids.clear()).await;
    registry::revoke_session(&sid).await.unwrap();
}

pub(super) async fn run(router: &Router, admin: (&str, &str)) {
    tokio::time::timeout(Duration::from_secs(20), async {
        registration_races(router, admin).await;
        publication_races(router, admin).await;
    })
    .await
    .expect("session race tests timed out");
}
