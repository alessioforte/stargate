use ctx::IssueRequest;

pub(crate) const PRIVATE_KEY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/crates/ctx/tests/fixtures/private.pem"
));
pub(crate) const PUBLIC_KEY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/crates/ctx/tests/fixtures/public.pem"
));
pub(super) const ROTATION_NEXT_PRIVATE_KEY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/crates/ctx/tests/fixtures/rotation-next-private.pem"
));
pub(super) const PUBLIC_JWK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/crates/ctx/tests/fixtures/public.jwk.json"
));

pub(super) fn anonymous_issue_request() -> IssueRequest {
    IssueRequest {
        audience: "urn:stargate:service:orders".to_owned(),
        subject: None,
        context: ctx::StargateContext {
            v: ctx::VERSION,
            actor: ctx::Actor {
                actor_type: ctx::ActorType::Anonymous,
            },
            authentication: ctx::Authentication {
                kind: ctx::AuthenticationKind::None,
                sid: None,
                auth_time: None,
            },
            organization: None,
            request: ctx::RequestContext {
                id: "01JZ000000000000000000000R".to_owned(),
                trace_id: None,
                method: "POST".to_owned(),
                path: "/v1/orders".to_owned(),
                original_path: "/api/orders".to_owned(),
                client_ip: None,
                user_agent: None,
            },
            route: ctx::RouteContext {
                router: "orders-write".to_owned(),
                service: "orders".to_owned(),
                policy_revision: None,
            },
            dispatch: ctx::DispatchContext {
                kind: ctx::DispatchKind::Primary,
                attempt: 1,
            },
        },
    }
}
