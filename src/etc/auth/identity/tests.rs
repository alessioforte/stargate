use super::*;
use crate::etc::auth::subject::{Subject, SubjectType};
use serde_json::json;

#[test]
fn verified_jwt_identity_keeps_only_validated_session_metadata() {
    let mut subject = Subject::new(
        "01JZ000000000000000000000A".to_owned(),
        SubjectType::User,
        Some(json!({ "secret": "must-not-propagate" })),
    );
    subject.org_id = Some("01JZ000000000000000000000Z".to_owned());
    subject.org_role = Some("admin".to_owned());

    let identity = VerifiedIdentity::jwt(
        subject,
        "01JZ000000000000000000000B".to_owned(),
        Some(1_784_473_000),
    )
    .unwrap();

    assert_eq!(identity.auth_kind(), AuthKind::Jwt);
    assert_eq!(identity.session_id(), Some("01JZ000000000000000000000B"));
    assert_eq!(identity.auth_time(), Some(1_784_473_000));
    assert_eq!(
        identity.subject().unwrap().attrs["secret"],
        "must-not-propagate"
    );
    assert!(!format!("{identity:?}").contains("must-not-propagate"));
}

#[test]
fn verified_api_key_identity_has_no_session_metadata() {
    let subject = Subject::new(
        "01JZ000000000000000000000K".to_owned(),
        SubjectType::ApiKey,
        None,
    );
    let identity = VerifiedIdentity::api_key(subject).unwrap();

    assert_eq!(identity.auth_kind(), AuthKind::ApiKey);
    assert_eq!(identity.session_id(), None);
    assert_eq!(identity.auth_time(), None);
}

#[test]
fn verified_oauth_identity_keeps_session_and_audience() {
    let subject = Subject::new(
        "01JZ000000000000000000000A".to_owned(),
        SubjectType::User,
        None,
    );
    let identity = VerifiedIdentity::oauth(
        subject,
        "01JZ000000000000000000000B".to_owned(),
        1_784_473_000,
        Some("gateway".to_owned()),
    )
    .unwrap();

    assert_eq!(identity.auth_kind(), AuthKind::OAuth);
    assert_eq!(identity.session_id(), Some("01JZ000000000000000000000B"));
    assert_eq!(identity.auth_time(), Some(1_784_473_000));
    assert_eq!(identity.oauth_audience(), Some("gateway"));
}

#[test]
fn anonymous_identity_has_no_subject_or_session() {
    let identity = VerifiedIdentity::anonymous();

    assert_eq!(identity.auth_kind(), AuthKind::Anonymous);
    assert!(identity.subject().is_none());
    assert_eq!(identity.session_id(), None);
    assert_eq!(identity.auth_time(), None);
}

#[test]
fn authentication_kind_must_match_subject_type() {
    let user = Subject::new("user".to_owned(), SubjectType::User, None);
    let api_key = Subject::new("key".to_owned(), SubjectType::ApiKey, None);

    assert!(VerifiedIdentity::api_key(user).is_none());
    assert!(VerifiedIdentity::jwt(api_key, "sid".to_owned(), None).is_none());
}
