use std::sync::Arc;

use ctx::{
    Actor, ActorType, Authentication, AuthenticationKind, Clock, ContextSigner, ContextVerifier,
    DispatchContext, DispatchIdSource, DispatchKind, ExpectedRequest, IssueRequest, Organization,
    RequestContext, RouteContext, SignerConfig, StargateContext, StaticKeyResolver, VerifierConfig,
};

pub const ISSUER: &str = "https://auth.example.com/internal-context";
pub const AUDIENCE: &str = "urn:stargate:service:orders";
pub const KEY_ID: &str = "stargate-internal-test";
pub const NOW: i64 = 1_784_474_100;
pub const DISPATCH_ID: &str = "01JZ000000000000000000000J";
pub const REQUEST_ID: &str = "01JZ000000000000000000000R";
pub const USER_ID: &str = "01JZ000000000000000000000A";
pub const API_KEY_ID: &str = "01JZ000000000000000000000C";
pub const SESSION_ID: &str = "01JZ000000000000000000000B";
pub const ORGANIZATION_ID: &str = "01JZ000000000000000000000Z";
pub const PRIVATE_KEY: &[u8] = include_bytes!("../fixtures/private.pem");
pub const PUBLIC_KEY: &[u8] = include_bytes!("../fixtures/public.pem");

#[derive(Debug)]
pub struct FixedClock(pub i64);

impl Clock for FixedClock {
    fn unix_timestamp(&self) -> i64 {
        self.0
    }
}

#[derive(Debug)]
pub struct FixedId(pub &'static str);

impl DispatchIdSource for FixedId {
    fn next_dispatch_id(&self) -> String {
        self.0.to_owned()
    }
}

pub fn signer() -> ContextSigner {
    ContextSigner::with_sources(
        SignerConfig::new(ISSUER, KEY_ID, 30).unwrap(),
        PRIVATE_KEY,
        Arc::new(FixedClock(NOW)),
        Arc::new(FixedId(DISPATCH_ID)),
    )
    .unwrap()
}

pub fn verifier(now: i64) -> ContextVerifier {
    let resolver = StaticKeyResolver::from_rsa_pem(KEY_ID, PUBLIC_KEY).unwrap();
    ContextVerifier::with_clock(
        VerifierConfig::new(ISSUER, AUDIENCE, 5).unwrap(),
        Arc::new(resolver),
        Arc::new(FixedClock(now)),
    )
}

pub fn expected() -> ExpectedRequest<'static> {
    ExpectedRequest {
        method: "POST",
        encoded_path: "/v1/orders",
        request_id: REQUEST_ID,
    }
}

pub fn user_request() -> IssueRequest {
    IssueRequest {
        audience: AUDIENCE.to_owned(),
        subject: Some(USER_ID.to_owned()),
        context: StargateContext {
            v: 1,
            actor: Actor {
                actor_type: ActorType::User,
            },
            authentication: Authentication {
                kind: AuthenticationKind::Jwt,
                sid: Some(SESSION_ID.to_owned()),
                auth_time: Some(1_784_473_000),
            },
            organization: Some(Organization {
                id: ORGANIZATION_ID.to_owned(),
                role: Some("admin".to_owned()),
            }),
            request: RequestContext {
                id: REQUEST_ID.to_owned(),
                trace_id: Some("4bf92f3577b34da6a3ce929d0e0e4736".to_owned()),
                method: "POST".to_owned(),
                path: "/v1/orders".to_owned(),
                original_path: "/api/orders".to_owned(),
                client_ip: Some("203.0.113.10".to_owned()),
                user_agent: Some("example-client/1.0".to_owned()),
            },
            route: RouteContext {
                router: "orders-write".to_owned(),
                service: "orders".to_owned(),
                policy_revision: Some(
                    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        .to_owned(),
                ),
            },
            dispatch: DispatchContext {
                kind: DispatchKind::Primary,
                attempt: 1,
            },
        },
    }
}

pub fn api_key_request() -> IssueRequest {
    let mut request = user_request();
    request.subject = Some(API_KEY_ID.to_owned());
    request.context.actor.actor_type = ActorType::ApiKey;
    request.context.authentication = Authentication {
        kind: AuthenticationKind::ApiKey,
        sid: None,
        auth_time: None,
    };
    request.context.organization = None;
    request.context.dispatch = DispatchContext {
        kind: DispatchKind::Shadow,
        attempt: 2,
    };
    request
}

pub fn anonymous_request() -> IssueRequest {
    let mut request = user_request();
    request.subject = None;
    request.context.actor.actor_type = ActorType::Anonymous;
    request.context.authentication = Authentication {
        kind: AuthenticationKind::None,
        sid: None,
        auth_time: None,
    };
    request.context.organization = None;
    request.context.request.trace_id = None;
    request.context.request.client_ip = None;
    request.context.request.user_agent = None;
    request.context.route.policy_revision = None;
    request
}
