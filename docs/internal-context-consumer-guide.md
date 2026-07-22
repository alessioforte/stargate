# Internal Context Consumer Integration Guide

Status: integration guide for contract version 1. This repository does not
implement or deploy any external microservice. A Stargate upstream is enabled
only after its owner supplies the evidence described below.

The normative protocol reference is
[`internal-context-contract-v1.md`](internal-context-contract-v1.md). The
standalone Rust SDK is named `ctx-sdk` and is maintained outside this
repository. It can be copied as one crate and has no dependency on Stargate or
the issuer-side `ctx` crate. Golden tokens, decoded claims, and public keys are
included in the SDK's `tests/fixtures/` directory.

## Integration outcome

A correctly integrated microservice has one protected boundary that:

1. accepts requests only from the intended internal network path;
2. requires exactly one `stargate-context` and `x-request-id` header;
3. verifies the signature, issuer, audience, time, method, encoded path, and
   request id before application routing or authorization;
4. gives handlers only a typed `TrustedInternalContext`;
5. never falls back to an edge bearer token, API key, or cookie; and
6. proves persisted organization ownership before organization-scoped
   authorization or audit.

## 1. Add the SDK

Obtain the complete `ctx-sdk` crate from its separately maintained source and
place it in the microservice repository, for example under `vendor/ctx-sdk`,
then add:

```toml
[dependencies]
ctx-sdk = { path = "vendor/ctx-sdk", features = ["axum"] }
```

Do not copy `crates/ctx`. It contains Stargate's signer-side implementation and
is not a consumer dependency. A consumer never receives a private signing key.

## 2. Configure one trust boundary

Use configuration owned by the microservice deployment, not request data:

| Setting | Required value |
| --- | --- |
| Issuer | Exact value of Stargate `INTERNAL_CONTEXT_ISSUER` |
| Audience | One exact logical service audience, such as `urn:stargate:service:orders` |
| JWKS URL | Trusted Stargate base URL plus `/.well-known/stargate-context-jwks.json` |
| Clock skew | Normally 5 seconds; never more than 30 seconds |
| JWKS cache age | Bounded deployment value; normally 60 seconds |
| Refresh cooldown | Positive and no longer than cache age; normally 1 second |

Do not accept audience lists, wildcards, suffix matching, OAuth/OIDC keys, or a
JWKS URL supplied by the token. The audience is a stable service identity, not
a URL, hostname, pod name, router name, or environment-specific instance id.

The SDK deliberately does not choose an HTTP client. The microservice supplies
an implementation of `JwksSource::fetch` that returns the complete JWKS body.
That implementation must enforce TLS, an allowlisted URL, connection and
request timeouts, and a response-size limit no greater than 65,536 bytes. Do
not follow redirects to an untrusted origin.

Build and preload the verifier before serving protected traffic:

```rust,ignore
use std::sync::Arc;
use ctx_sdk::{
    CachedJwksResolver, ContextVerifier, JwksCacheConfig, JwksSource,
    VerifierConfig,
};

let source: Arc<dyn JwksSource> = Arc::new(service_owned_jwks_source);
let cache = JwksCacheConfig::new(60, 1)?;
let resolver = CachedJwksResolver::new(source, cache);

// Startup fails if no valid initial key set can be loaded.
resolver.refresh()?;

let verifier = ContextVerifier::new(
    VerifierConfig::new(
        "https://auth.example.com/internal-context",
        "urn:stargate:service:orders",
        5,
    )?,
    Arc::new(resolver),
);
```

The resolver serializes refreshes, bounds document and key counts, refreshes an
unknown `kid` at most once per cooldown, and never uses an expired
last-known-good key set after refresh failure.

## 3. Protect the Axum routes

Install the middleware outside path normalization or rewriting. Put every
protected route and its fallback inside the protected router; keep health and
readiness routes on a separate public router if required.

```rust,ignore
use axum::{middleware, routing::post, Router};
use ctx_sdk::axum::{InternalContextState, require_internal_context};

let state = InternalContextState::new(verifier);

let protected = Router::new()
    .route("/v1/orders", post(create_order))
    .fallback(protected_not_found)
    .layer(middleware::from_fn_with_state(
        state,
        require_internal_context,
    ));

let app = Router::new()
    .route("/health", axum::routing::get(health))
    .merge(protected);
```

The middleware verifies once, removes `stargate-context`, replaces
`x-request-id` with the signed value, and inserts `TrustedInternalContext` into
request extensions. All failures return the same generic `401` response with
`Cache-Control: no-store`. Handlers must not parse compact tokens or construct
trusted context from JSON or plain headers.

## 4. Use the typed context

Handlers extract `TrustedInternalContext`. Relevant trusted fields are:

| API | Meaning | Appropriate use |
| --- | --- | --- |
| `subject()` | User id or API-key id; absent for anonymous | Actor identity |
| `context().actor.actor_type` | `User`, `ApiKey`, or `Anonymous` | Select service authorization rules |
| `context().authentication` | Verified gateway authentication shape | Audit/security context |
| `context().organization` | Active organization assertion and optional role | Input to an ownership check, never ownership proof |
| `context().request` | Signed request/correlation facts | Audit correlation |
| `context().route` | Gateway routing diagnostics | Diagnostics only |
| `context().dispatch` | `Primary` or `Shadow`, plus attempt | Enforce the service's shadow policy |

```rust,ignore
use ctx_sdk::{ActorType, DispatchKind};
use ctx_sdk::axum::TrustedInternalContext;

async fn create_order(context: TrustedInternalContext) {
    match context.context().actor.actor_type {
        ActorType::User => {
            let user_id = context.subject().expect("verified user has a subject");
            // Apply the service's user authorization policy.
        }
        ActorType::ApiKey => {
            let api_key_id = context.subject().expect("verified API key has a subject");
            // Apply the service's API-key authorization policy.
        }
        ActorType::Anonymous => {
            // Allow only operations explicitly public in this service.
        }
    }

    if context.context().dispatch.kind == DispatchKind::Shadow {
        // Reject unsafe mutations or route them to isolated shadow handling.
    }
}
```

The context authenticates gateway assertions; it does not decide whether an
actor may perform a business action. Authorization remains service-owned.

## 5. Prove organization ownership and build audit data

The signed organization means “active organization selected by Stargate.” It
does not prove that the affected resource belongs to that organization. Load
the resource from the service database, then compare its persisted owner:

```rust,ignore
use ctx_sdk::organization_from_persisted;

let audit = context.audit_input();
let resource = repository.load_order(order_id).await?;
let organization = organization_from_persisted(
    resource.organization_id.as_deref(),
    audit.asserted_organization_id(),
)?;

// Only now may organization role/policy be applied to this resource.
authorize_for_organization(organization.organization_id(), &context)?;
```

Missing ownership, a missing assertion, an invalid persisted id, or a mismatch
must fail. Never fall back to application scope.

`audit_input()` supplies only verified actor and request correlation facts. The
microservice still owns the action, resource, outcome, before/after snapshots,
metadata, service/event ids, disclosure rules, and transactional outbox. The
business mutation and audit outbox insert remain one local transaction. Never
store the compact token or complete decoded payload in an audit event.

## 6. Trace and WebSocket behavior

Local W3C trace context is authoritative for telemetry. The signed trace id is
authoritative for audit correlation. A mismatch is a bounded diagnostic signal
and does not by itself authorize or reject a request. Diagnostics must not log
tokens, decoded claims, header values, key ids, or signature details.

For WebSockets, the context protects only the HTTP upgrade handshake. Store the
verified typed context in connection state. Token expiry after a successful
upgrade does not close the connection, and frames do not carry another context.
The token must never be relayed unchanged to another service.

## 7. Required consumer tests

Before Stargate enables the upstream, its owner records evidence that the
deployed boundary:

- accepts the golden user, API-key, and anonymous shapes for the configured
  audience;
- rejects a missing or duplicate context or request-id header;
- rejects malformed, oversized, expired, future, wrongly signed, unknown-key,
  wrong-issuer, wrong-audience, wrong-method, wrong-path, and wrong-request-id
  contexts;
- returns one generic external error and exposes no verification details;
- makes the raw compact header unavailable to handlers;
- rejects direct network requests that do not pass through Stargate;
- rejects missing or mismatched persisted organization ownership;
- enforces a safe policy for `shadow` dispatches; and
- accepts a new rotation key through unknown-`kid` refresh while the retiring
  key still works, then fails closed on an expired cache plus refresh failure.

The receiving service should also confirm that a context-enabled request has
no `authorization`, `proxy-authorization`, `x-api-key`, Stargate `jwt` cookie,
recognized authentication query token, baggage, or client-authored internal
context. Unrelated well-formed cookies and non-auth query pairs must retain
their intended semantics.

## 8. Enable without a `dual` mode

The safest zero-downtime rollout is a new protected listener, port, or service
revision:

1. Choose one audience and add a pending entry to
   [`internal-context-upstream-inventory.md`](internal-context-upstream-inventory.md).
2. Deploy the new consumer boundary so it requires valid context from its first
   reachable request. Keep it inaccessible to untrusted networks.
3. Run the tests above directly against that boundary and record the consumer
   release, network controls, ownership result, and rotation result.
4. Configure Stargate to target that protected revision and enable its audience
   in the same configuration change:

   ```yaml
   http:
     upstreams:
       orders:
         targets:
           - url: https://orders-internal:8443
         internal_context:
           audience: urn:stargate:service:orders
   ```

5. Verify normal traffic, credential removal, correlation, authorization,
   audit, failover, mirror, and WebSocket shapes that the upstream actually
   uses. Mark the inventory entry enabled only after the checks pass.

If a separate protected revision is impossible, coordinate the consumer and
Stargate changes in a maintenance window. Never enable Stargate first against
a consumer that ignores the context, and never preserve availability by
falling back to reusable edge credentials.

Rollback means restoring a previously compatible Stargate/consumer pair or
disabling the route until compatibility is restored. Removing only the
consumer check or forwarding original credentials is not a valid rollback.

## 9. Key rotation

1. Publish old and new public keys together in Stargate's internal JWKS.
2. Confirm the consumer accepts a token using the new `kid` through an
   unknown-key refresh while it still accepts the retiring key.
3. Switch Stargate to the new private key and `kid`.
4. Retain the old public key through token lifetime, maximum clock skew, and
   downstream cache overlap.
5. Exercise fetch failure and unknown-key rejection; neither may use an edge
   credential or an expired cached key.

Every enabled upstream needs its own completed inventory entry. Test fixtures,
example YAML, and the repository loopback reference do not count as evidence
that an external microservice is deployed.
