use super::*;
use crate::api::gateway::execution::replay::ReplayRequest;
use crate::etc::internal_context::test_support::{PRIVATE_KEY, PUBLIC_KEY};
use crate::etc::{auth::identity::VerifiedIdentity, request_context::RequestContext};
use axum::body::Body;
use chrono::Utc;
use ctx::{
    ContextSigner, ContextVerifier, ExpectedRequest, SignerConfig, StaticKeyResolver,
    VerifierConfig,
};
use hyper::body::Bytes;
use std::collections::HashSet;

const ISSUER: &str = "https://stargate.test/internal-context";
const AUDIENCE: &str = "urn:stargate:service:orders";
const FALLBACK_AUDIENCE: &str = "urn:stargate:service:fallback";
const SHADOW_AUDIENCE: &str = "urn:stargate:service:shadow";
const KEY_ID: &str = "stargate-internal-test";
const REQUEST_ID: &str = "01JZ000000000000000000000R";

fn runtime() -> Arc<InternalContextRuntime> {
    let signer =
        ContextSigner::from_rsa_pem(SignerConfig::new(ISSUER, KEY_ID, 30).unwrap(), PRIVATE_KEY)
            .unwrap();
    InternalContextRuntime::from_signer_for_test(signer)
}

fn draft() -> Arc<PropagationDraft> {
    let request = RequestContext::new(REQUEST_ID.to_owned(), Utc::now(), None, None, None);
    Arc::new(
        PropagationDraft::build(
            &VerifiedIdentity::anonymous(),
            &request,
            "POST",
            "/orders",
            "/orders",
            ctx::RouteContext {
                router: "orders-write".to_owned(),
                service: "orders".to_owned(),
                policy_revision: None,
            },
        )
        .unwrap(),
    )
}

#[test]
fn auth_query_sanitizer_decodes_only_keys_and_preserves_every_other_pair() {
    let query =
        "a=1&%74oken=secret&encoded=%2Fkeep%2F&access%5Ftoken=x&JWT=keep&j%77t=y&empty=&flag";

    let sanitized = sanitize_auth_query(query).unwrap();

    assert_eq!(sanitized, "a=1&encoded=%2Fkeep%2F&JWT=keep&empty=&flag");
}

#[test]
fn malformed_percent_encoding_in_a_query_key_fails_closed() {
    assert!(sanitize_auth_query("safe=1&tok%en=secret").is_err());
    assert!(sanitize_auth_query("safe=1&token%=secret").is_err());
}

#[test]
fn internal_uri_sanitizer_preserves_authority_path_and_safe_encoding() {
    let uri: http::Uri = "http://orders.test/v1/a%2Fb?z=%2F&jwt=secret&a=1"
        .parse()
        .unwrap();

    let sanitized = sanitize_internal_uri(&uri).unwrap();

    assert_eq!(sanitized, "http://orders.test/v1/a%2Fb?z=%2F&a=1");
}

#[tokio::test]
async fn concurrent_replay_clones_mint_unique_primary_and_shadow_contexts() {
    let runtime = runtime();
    let draft = draft();
    let replay = ReplayRequest {
        method: http::Method::POST,
        version: http::Version::HTTP_11,
        headers: http::HeaderMap::new(),
        body: Bytes::from_static(b"payload"),
    };
    let expected_attempts = [
        (DispatchKind::Primary, 1_u16, AUDIENCE),
        (DispatchKind::Primary, 2_u16, FALLBACK_AUDIENCE),
        (DispatchKind::Shadow, 1_u16, SHADOW_AUDIENCE),
        (DispatchKind::Shadow, 2_u16, FALLBACK_AUDIENCE),
    ];

    let mut tasks = Vec::new();
    for (kind, attempt, audience) in expected_attempts {
        let runtime = Arc::clone(&runtime);
        let draft = Arc::clone(&draft);
        let replay = replay.clone();
        tasks.push(tokio::spawn(async move {
            let dispatch = InternalDispatch::new(
                "orders",
                audience,
                Some(&draft),
                Some(&runtime),
                crate::api::gateway::upstream::attempt::DispatchAttempt {
                    kind,
                    number: attempt,
                },
            )
            .unwrap();
            let mut request: http::Request<Body> = replay
                .build("http://orders.test/orders?jwt=secret&safe=1")
                .unwrap();
            dispatch.prepare(&mut request).await.unwrap();
            assert_eq!(request.uri().query(), Some("safe=1"));
            assert_eq!(
                request
                    .headers()
                    .get_all(&INTERNAL_CONTEXT_HEADER)
                    .iter()
                    .count(),
                1
            );
            request.headers()[INTERNAL_CONTEXT_HEADER]
                .to_str()
                .unwrap()
                .to_owned()
        }));
    }

    let mut dispatch_ids = HashSet::new();
    for (task, (kind, attempt, audience)) in tasks.into_iter().zip(expected_attempts) {
        let token = task.await.unwrap();
        let resolver = StaticKeyResolver::from_rsa_pem(KEY_ID, PUBLIC_KEY).unwrap();
        let verifier = ContextVerifier::new(
            VerifierConfig::new(ISSUER, audience, 5).unwrap(),
            Arc::new(resolver),
        );
        let trusted = verifier
            .verify(
                &token,
                ExpectedRequest {
                    method: "POST",
                    encoded_path: "/orders",
                    request_id: REQUEST_ID,
                },
            )
            .unwrap();
        assert_eq!(trusted.context().dispatch.kind, kind);
        assert_eq!(trusted.context().dispatch.attempt, attempt);
        assert_eq!(trusted.context().request.id, REQUEST_ID);
        assert!(dispatch_ids.insert(trusted.dispatch_id().to_owned()));
    }
    assert_eq!(dispatch_ids.len(), expected_attempts.len());
}
