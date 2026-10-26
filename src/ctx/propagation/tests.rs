use super::*;
use crate::etc::auth::{
    identity::VerifiedIdentity,
    subject::{Subject, SubjectType},
};
use chrono::Utc;
use serde_json::json;

fn request_context(user_agent: Option<String>) -> RequestContext {
    RequestContext::new(
        "01JZ000000000000000000000R".to_owned(),
        Utc::now(),
        Some("203.0.113.10".parse().unwrap()),
        user_agent,
        Some("4bf92f3577b34da6a3ce929d0e0e4736".to_owned()),
    )
}

#[test]
fn user_draft_maps_only_contract_identity_and_request_facts() {
    let mut subject = Subject::new(
        "01JZ000000000000000000000A".to_owned(),
        SubjectType::User,
        Some(json!({
            "secret": "must-not-propagate",
            "email": "private@example.test",
            "quota": "large"
        })),
    );
    subject.org_id = Some("01JZ000000000000000000000Z".to_owned());
    subject.org_role = Some("admin".to_owned());
    let identity = VerifiedIdentity::test_jwt(
        subject,
        "01JZ000000000000000000000B".to_owned(),
        Some(1_784_473_000),
    );

    let draft = PropagationDraft::build(
        &identity,
        &request_context(Some("example-client/1.0".to_owned())),
        "POST",
        "/v1/orders",
        "/api/orders",
        ctx::RouteContext {
            router: "orders-write".to_owned(),
            service: "orders".to_owned(),
            policy_revision: Some("sha256:revision".to_owned()),
        },
    )
    .unwrap();

    assert_eq!(draft.subject.as_deref(), Some("01JZ000000000000000000000A"));
    assert_eq!(draft.actor.actor_type, ActorType::User);
    assert_eq!(draft.authentication.kind, AuthenticationKind::Jwt);
    assert_eq!(
        draft.authentication.sid.as_deref(),
        Some("01JZ000000000000000000000B")
    );
    assert_eq!(draft.authentication.auth_time, Some(1_784_473_000));
    assert_eq!(
        draft.organization.as_ref().unwrap().id,
        "01JZ000000000000000000000Z"
    );
    assert_eq!(
        draft.organization.as_ref().unwrap().role.as_deref(),
        Some("admin")
    );
    assert_eq!(draft.request.id, "01JZ000000000000000000000R");
    assert_eq!(draft.request.client_ip.as_deref(), Some("203.0.113.10"));
    assert_eq!(
        draft.request.trace_id.as_deref(),
        Some("4bf92f3577b34da6a3ce929d0e0e4736")
    );
    assert_eq!(draft.request.path, "/v1/orders");
    assert_eq!(draft.request.original_path, "/api/orders");

    let visible = serde_json::to_string(&(
        draft.subject.as_deref(),
        &draft.actor,
        &draft.authentication,
        draft.organization.as_ref(),
        &draft.request,
        &draft.route,
    ))
    .unwrap();
    assert!(!visible.contains("must-not-propagate"));
    assert!(!visible.contains("private@example.test"));
    assert!(!visible.contains("quota"));
    assert_eq!(format!("{draft:?}"), "PropagationDraft([redacted])");
}

#[test]
fn issuance_binds_leaf_dispatch_and_the_exact_final_path() {
    let draft = PropagationDraft::build(
        &VerifiedIdentity::anonymous(),
        &request_context(None),
        "POST",
        "/orders",
        "/api/orders",
        ctx::RouteContext {
            router: "orders-write".to_owned(),
            service: "orders".to_owned(),
            policy_revision: None,
        },
    )
    .unwrap();

    let issued = draft
        .issue_request(
            "urn:stargate:service:orders",
            DispatchKind::Primary,
            1,
            "POST",
            "/internal/v1/orders",
        )
        .unwrap();

    assert_eq!(issued.audience, "urn:stargate:service:orders");
    assert_eq!(issued.context.request.path, "/internal/v1/orders");
    assert_eq!(issued.context.request.original_path, "/api/orders");
    assert_eq!(issued.context.dispatch.kind, DispatchKind::Primary);
    assert_eq!(issued.context.dispatch.attempt, 1);
    assert!(issued.subject.is_none());
    assert!(
        draft
            .issue_request(
                "urn:stargate:service:orders",
                DispatchKind::Primary,
                1,
                "GET",
                "/internal/v1/orders",
            )
            .is_err()
    );
}

#[test]
fn api_key_and_anonymous_shapes_are_explicit() {
    let mut subject = Subject::new(
        "01JZ000000000000000000000K".to_owned(),
        SubjectType::ApiKey,
        None,
    );
    subject.org_id = Some("01JZ000000000000000000000Z".to_owned());
    let api_key = VerifiedIdentity::test_api_key(subject);
    let api_key_draft = PropagationDraft::build(
        &api_key,
        &request_context(None),
        "GET",
        "/orders",
        "/orders",
        ctx::RouteContext {
            router: "orders-read".to_owned(),
            service: "orders".to_owned(),
            policy_revision: None,
        },
    )
    .unwrap();
    assert_eq!(api_key_draft.actor.actor_type, ActorType::ApiKey);
    assert_eq!(
        api_key_draft.authentication.kind,
        AuthenticationKind::ApiKey
    );
    assert!(api_key_draft.authentication.sid.is_none());

    let anonymous = PropagationDraft::build(
        &VerifiedIdentity::anonymous(),
        &request_context(None),
        "GET",
        "/public",
        "/public",
        ctx::RouteContext {
            router: "public".to_owned(),
            service: "public-service".to_owned(),
            policy_revision: None,
        },
    )
    .unwrap();
    assert_eq!(anonymous.actor.actor_type, ActorType::Anonymous);
    assert_eq!(anonymous.authentication.kind, AuthenticationKind::None);
    assert!(anonymous.subject.is_none());
    assert!(anonymous.organization.is_none());
}

#[test]
fn request_payload_query_cookie_and_raw_headers_cannot_supply_draft_facts() {
    let attacker_body = json!({
        "actor": "user",
        "organization": "attacker-org",
        "role": "owner",
        "request_id": "attacker-request",
        "client_ip": "198.51.100.99",
        "trace_id": "ffffffffffffffffffffffffffffffff"
    });
    let attacker_query = "actor=user&organization=attacker-org&request_id=attacker-request";
    let attacker_cookie = "actor=user; organization=attacker-org; role=owner";
    let attacker_header = "attacker-signed-context";

    let draft = PropagationDraft::build(
        &VerifiedIdentity::anonymous(),
        &request_context(None),
        "GET",
        "/public",
        "/public",
        ctx::RouteContext {
            router: "public".to_owned(),
            service: "public-service".to_owned(),
            policy_revision: Some("sha256:trusted".to_owned()),
        },
    )
    .unwrap();
    let visible = serde_json::to_string(&(
        draft.subject.as_deref(),
        &draft.actor,
        &draft.authentication,
        draft.organization.as_ref(),
        &draft.request,
        &draft.route,
    ))
    .unwrap();

    for untrusted in [
        attacker_body.to_string(),
        attacker_query.to_owned(),
        attacker_cookie.to_owned(),
        attacker_header.to_owned(),
        "attacker-org".to_owned(),
        "attacker-request".to_owned(),
        "198.51.100.99".to_owned(),
        "ffffffffffffffffffffffffffffffff".to_owned(),
    ] {
        assert!(!visible.contains(&untrusted));
    }
    assert_eq!(draft.actor.actor_type, ActorType::Anonymous);
    assert_eq!(draft.request.id, "01JZ000000000000000000000R");
    assert_eq!(draft.request.client_ip.as_deref(), Some("203.0.113.10"));
}

#[test]
fn draft_user_agent_is_truncated_on_a_utf8_boundary() {
    let context = request_context(Some(format!("{}💫", "a".repeat(511))));
    let draft = PropagationDraft::build(
        &VerifiedIdentity::anonymous(),
        &context,
        "GET",
        "/public",
        "/public",
        ctx::RouteContext {
            router: "public".to_owned(),
            service: "public-service".to_owned(),
            policy_revision: None,
        },
    )
    .unwrap();
    let user_agent = draft.request.user_agent.as_deref().unwrap();

    assert_eq!(user_agent.len(), 511);
    assert!(user_agent.is_char_boundary(user_agent.len()));
    assert!(!user_agent.contains('💫'));
    assert!(context.user_agent().unwrap().contains('💫'));
}

#[test]
fn owned_route_facts_are_validated_before_capture() {
    let route = RouteContext {
        router: "orders-read".to_owned(),
        service: "orders".to_owned(),
        policy_revision: Some("sha256:revision".to_owned()),
    };
    let identity = VerifiedIdentity::anonymous();
    let context = request_context(None);
    for invalid in [
        RouteContext {
            router: " ".to_owned(),
            ..route.clone()
        },
        RouteContext {
            service: "".to_owned(),
            ..route.clone()
        },
        RouteContext {
            policy_revision: Some("x".repeat(MAX_NAME_BYTES + 1)),
            ..route.clone()
        },
    ] {
        assert_eq!(
            PropagationDraft::build(
                &identity,
                &context,
                "GET",
                "/orders",
                "/orders",
                invalid.clone()
            ),
            Err(PropagationDraftError::Route)
        );
        // Request validation retains precedence when both inputs are invalid.
        assert_eq!(
            PropagationDraft::build(
                &identity,
                &context,
                "GET",
                "/orders?token=x",
                "/orders",
                invalid
            ),
            Err(PropagationDraftError::Request)
        );
    }
    let draft = PropagationDraft::build(
        &identity,
        &context,
        "GET",
        "/orders",
        "/orders",
        route.clone(),
    )
    .unwrap();
    assert_eq!(draft.route, route);
}
