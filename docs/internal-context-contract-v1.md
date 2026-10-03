# Stargate Internal Request Context Protocol

Protocol version: 1

This reference describes the signed request context emitted by Stargate for an
upstream configured with `internal_context`. It defines the HTTP headers, token
format, claims, validation rules, security boundaries, and compatibility
behavior implemented by Stargate.

Related documentation:

- [Config v1](config-v1.md) explains how to enable internal context
  for an upstream.
- [Consumer integration guide](internal-context-consumer-guide.md) shows how a
  receiving service integrates the verifier boundary.
- [Operations runbook](internal-context-operations.md) covers key provisioning,
  rotation, monitoring, and recovery.

## Overview

The protocol gives a service authenticated request, organization, routing, and
audit input facts without requiring that service to consume or replay the
client's original Stargate access token, API key, or session cookie.

Version 1 uses three independent channels:

- W3C `traceparent` and `tracestate` for distributed tracing;
- `x-request-id` for plain operational correlation; and
- `stargate-context` for a compact, asymmetrically signed JWT containing facts
  that become trusted only after complete consumer validation.

The signed context is not a complete authorization decision or audit event. A
consumer remains responsible for endpoint authorization, persisted resource
ownership, business semantics, and transactional audit production.

Normative terms such as MUST, MUST NOT, REQUIRED, SHOULD, and MAY describe the
requirements for interoperating with protocol version 1.

## Security Model and Trust Boundaries

### External client to Stargate

The client and every client-authored header are untrusted. Stargate authenticates
supported credentials, selects the active organization from verified state,
applies its configured policies, removes any client-supplied
`stargate-context`, and replaces any client-supplied `x-request-id` before
gateway header routing.

### Stargate to an internal service

Stargate is the only version 1 issuer. The service trusts a context only after
validating its signature, type, algorithm, key, issuer, audience, timestamps,
version, request binding, limits, and actor shape.

TLS is REQUIRED for internal transport; mTLS and network reachability controls
are strongly recommended. A JWS proves integrity and issuer possession of a
signing key, but does not encrypt its claims or stop same-endpoint replay during
its short validity window.

### Internal service to persisted business state

The signed organization is an assertion about the effective gateway identity.
It is not proof that a requested order, project, account, or other resource
belongs to that organization. The service MUST load the affected resource and
compare its persisted owner to the assertion before organization-scoped access
or audit construction.

### Key distribution

Internal-context signing keys and JWKS are separate from OAuth/OIDC/session
keys. A consumer selects a verification key only from its configured trusted
internal JWKS/key resolver and the signed `kid`. Token-provided key URLs or
embedded keys are never trusted.

## Header Contract

HTTP field names are case-insensitive. A consumer MUST handle their values as
specified below.

| Header | Issuer behavior | Consumer behavior | Security role |
| --- | --- | --- | --- |
| `stargate-context` | Remove all inbound occurrences; emit exactly one value for a configured internal dispatch | Require exactly one value and perform the full validation sequence | Trusted context after validation |
| `x-request-id` | Generate once per ingress request; replace the inbound value before routing and the outbound value before dispatch | Require exactly one value and exact equality with `stg.request.id` | Correlation; signed equality prevents spoofing |
| `traceparent` | Extract a valid parent and inject the active client span | Use normal W3C extraction | Telemetry only |
| `tracestate` | Propagate only with valid trace context | Use normal W3C extraction | Telemetry only |
| `baggage` | Do not forward to version 1 internal upstreams | Ignore for identity and authorization | None |
| `authorization` | Use for edge authentication, then remove from every configured internal dispatch | Do not use as the internal-context trust source | Edge authentication only |
| `x-api-key` | Use for edge authentication, then remove from every configured internal dispatch | Do not use as the internal-context trust source | Edge authentication only |
| `cookie` entry `jwt` | Use for edge authentication, then remove from every configured internal dispatch | Do not use as the internal-context trust source | Edge authentication only |

Stargate MUST remove `stargate-context` from upstream responses and MUST NOT
return it to an external caller. Consumers MUST redact both the compact token
and its complete decoded payload from logs, traces, metrics, errors, and crash
reports.

The maximum compact `stargate-context` field value is 4,096 bytes. Stargate
MUST check the final compact serialization before dispatch. A consumer MUST
reject a larger value before JSON or cryptographic processing.

## Compact Serialization and JOSE Header

Version 1 uses a JWT claims payload protected using compact JWS serialization.
The JOSE header is:

```json
{
  "alg": "RS256",
  "kid": "stargate-internal-current",
  "typ": "stargate-context+jwt"
}
```

Requirements:

- `typ` MUST be exactly `stargate-context+jwt`.
- `alg` MUST be exactly `RS256` in version 1.
- `kid` MUST be present, nonblank, and no more than 128 ASCII bytes.
- `none`, every `HS*` algorithm, and any other algorithm MUST be rejected.
- A key is used with one configured algorithm only.
- `jku`, `jwk`, `x5u`, and other token-directed key-selection parameters MUST
  be rejected.
- Unsupported critical JOSE parameters MUST be rejected.
- Internal-context keys MUST NOT be reused for any public or user-facing token
  type.

No other signing algorithm is part of protocol version 1. Supporting another
algorithm requires a separately versioned profile with algorithm-specific test
vectors and mutually exclusive verifier rules.

## Claims Schema

### Complete user example

All example entity/request identifiers below are valid uppercase ULIDs.

```json
{
  "iss": "https://auth.example.com/internal-context",
  "aud": "urn:stargate:service:orders",
  "iat": 1784474100,
  "exp": 1784474130,
  "jti": "01JZ000000000000000000000J",
  "sub": "01JZ000000000000000000000A",
  "stg": {
    "v": 1,
    "actor": {
      "type": "user"
    },
    "authentication": {
      "kind": "jwt",
      "sid": "01JZ000000000000000000000B",
      "auth_time": 1784473000
    },
    "organization": {
      "id": "01JZ000000000000000000000Z",
      "role": "admin"
    },
    "request": {
      "id": "01JZ000000000000000000000R",
      "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
      "method": "POST",
      "path": "/v1/orders",
      "original_path": "/api/orders",
      "client_ip": "203.0.113.10",
      "user_agent": "example-client/1.0"
    },
    "route": {
      "router": "orders-write",
      "service": "orders",
      "policy_revision": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    },
    "dispatch": {
      "kind": "primary",
      "attempt": 1
    }
  }
}
```

`jti` is the dispatch identifier. There is deliberately no second
`dispatch.id`; avoiding duplicate identifiers removes a mismatch case and
reduces the header size. `request.id` remains stable across every dispatch for
one ingress request, while `jti` changes for each primary, failover, and mirror
network attempt.

### Top-level claims

| Claim | Required | Type / limit | Meaning and validation |
| --- | --- | --- | --- |
| `iss` | Yes | Nonblank string, at most 256 bytes | Exact configured internal issuer; normally `${JWT_ISSUER}/internal-context` |
| `aud` | Yes | One nonblank string, at most 256 bytes | Exact stable logical audience configured for the selected leaf upstream; arrays are not valid in v1 |
| `iat` | Yes | NumericDate integer | Signing time |
| `exp` | Yes | NumericDate integer | Expiry; MUST be greater than `iat` |
| `jti` | Yes | Uppercase ULID | Unique id for this signed network dispatch |
| `sub` | Conditional | Uppercase Stargate entity ULID | Effective user/API-key id; absent for anonymous |
| `stg` | Yes | Object | Versioned private claims namespace |

Time rules:

- Default `exp - iat` is 30 seconds.
- `exp - iat` MUST NOT exceed 60 seconds.
- Default verifier clock skew is 5 seconds and MUST NOT be configured above 30
  seconds.
- A token whose `iat` is later than `now + skew` is invalid.
- A token whose `exp` is at or before `now - skew` is invalid.
- `nbf` is not part of version 1. If present, it is an unknown claim and the
  strict version 1 parser rejects it.

### `stg` namespace

| Field | Required | Type / limit | Meaning and validation |
| --- | --- | --- | --- |
| `v` | Yes | Integer exactly `1` | Protocol version |
| `actor` | Yes | Object | Effective authenticated actor |
| `authentication` | Yes | Object | Mechanism and verified session facts |
| `organization` | No | Object | Active organization assertion |
| `request` | Yes | Object | Bound request and audit input facts |
| `route` | Yes | Object | Gateway routing diagnostics |
| `dispatch` | Yes | Object | Primary/shadow and attempt semantics |

Version 1 objects use fixed fields. Unknown fields MUST be rejected. Optional
values are omitted rather than serialized as JSON `null`.

## Actor and Authentication Matrix

Only the following combinations are valid:

| `actor.type` | `authentication.kind` | `sub` | `sid` | Organization |
| --- | --- | --- | --- | --- |
| `user` | `jwt` | Required user ULID | Required session ULID | Optional; role required when present |
| `api_key` | `api_key` | Required API-key ULID | Absent | Optional; role optional because current bound API-key state may not carry a membership role |
| `anonymous` | `none` | Absent | Absent | Absent |

Rules:

- `actor` contains only `type`; the identified actor id is the standard `sub`.
- `auth_time` is optional for `user`/`jwt` and absent for other combinations.
- An organization id, when present, is an uppercase organization ULID.
- An organization role is nonblank and at most 256 UTF-8 bytes.
- Email, display name, nickname, profile fields, raw JWT claims, raw API keys,
  hashes, and arbitrary subject attributes are forbidden.
- New actor/authentication types require a new protocol version.
- On a public route, a request with no usable verified credential is represented
  as anonymous. The signed anonymous context does not attest that the caller
  supplied no malformed credential; it describes only the effective identity
  accepted by the gateway.

## Request Binding and Facts

### Required request fields

| Field | Type / limit | Rule |
| --- | --- | --- |
| `id` | Uppercase ULID | Generated by Stargate once for the ingress request |
| `method` | Nonblank ASCII method, at most 32 bytes | Exact, case-sensitive HTTP method sent upstream |
| `path` | Nonblank URI path, at most 2,048 UTF-8 bytes | Exact post-route-middleware path sent to the selected upstream |
| `original_path` | Nonblank URI path, at most 2,048 UTF-8 bytes | Path observed at gateway fallback before route-specific rewrites |

The consumer MUST compare its raw request method and path to the signed values
before framework routing, percent-decoding, slash collapsing, dot-segment
processing, or application normalization can change their representation.
Comparison is case-sensitive and performed on the encoded path form. Query and
fragment components are excluded.

The plain `x-request-id` header MUST be present exactly once and equal
`stg.request.id`. The consumer uses the signed value after verification.

### Optional request facts

| Field | Type / limit | Rule |
| --- | --- | --- |
| `trace_id` | 32 lowercase hexadecimal characters | Active W3C trace id used by Stargate, not raw `traceparent` bytes |
| `client_ip` | Canonical IPv4 or IPv6 text | Resolved by Stargate using the connection peer and `TRUSTED_PROXIES` policy |
| `user_agent` | UTF-8 string, at most 512 bytes | Truncated by the issuer only at a valid UTF-8 boundary |

Trace context is never an authorization input. If the locally extracted trace
id differs from the signed diagnostic trace id, the consumer records a bounded
diagnostic signal, uses the local trace context for telemetry, and uses the
signed trace id for audit correlation. The mismatch alone does not reject
authorization.

The compact JWS is not encrypted. Client IP and user agent may be personal or
sensitive data; they MUST remain on protected internal transport and the token
MUST be redacted from logs.

## Route and Dispatch Semantics

### Route

| Field | Required | Type / limit | Rule |
| --- | --- | --- | --- |
| `router` | Yes | Nonblank string, at most 128 bytes | Matched gateway router name |
| `service` | Yes | Nonblank string, at most 128 bytes | Root service named by the matched router |
| `policy_revision` | No | Nonblank string, at most 128 bytes | Diagnostic policy revision only |

Route facts show what Stargate selected. They are not downstream roles,
permissions, scopes, or proof that a particular resource operation is allowed.

### Dispatch

| Field | Required | Type | Rule |
| --- | --- | --- | --- |
| `kind` | Yes | `primary` or `shadow` | Mirrors are always `shadow`; ordinary and failover attempts are `primary` |
| `attempt` | Yes | Integer from 1 through 65,535 | One-based within the applicable primary or individual shadow execution plan |

Every actual upstream network attempt gets a new `jti`, even when the request
body and logical request id are replayed. A replay buffer MUST NOT contain a
previously signed `stargate-context`.

Each selected leaf upstream supplies its own audience. Therefore weighted
versions may intentionally share one logical audience, while a failover or
mirror may receive a different audience.

Shadow context is an authenticated signal, not automatic side-effect
protection. Every mirrored consumer MUST define safe shadow behavior. The
safest default for a service unable to isolate shadow writes is to reject
unsafe shadow methods.

For WebSockets, the context authenticates and binds the upstream HTTP upgrade
handshake. The verified typed context is fixed for the accepted connection;
expiry after a successful handshake does not terminate the socket. Message-level
reauthentication is outside version 1.

## Enabling Internal Context

Context behavior is configured on a physical upstream pool:

```yaml
http:
  upstreams:
    orders:
      internal_context:
        audience: urn:stargate:service:orders
      targets:
        - url: http://orders:8080
```

The configuration has two structural states and no rollout `mode`:

- When `internal_context` is absent, Stargate emits no JWS and preserves the
  upstream's existing forwarding behavior. Global gateway-owned header hygiene
  still applies: `stargate-context` is removed at ingress and from responses,
  and `x-request-id` is replaced with Stargate's generated value. A consumer
  MUST NOT interpret the absent header as a trusted anonymous context.
- When `internal_context` is present, `audience` is required and nonblank.
  Stargate emits the signed context, replaces gateway-owned request and trace
  correlation, removes baggage, and applies all credential-isolation rules
  below before every dispatch.

For a configured internal dispatch, Stargate:

- Removes all `authorization`, `proxy-authorization`, and `x-api-key` values.
- Removes the exact Stargate `jwt` cookie while preserving well-formed unrelated
  cookies.
- Removes query pairs whose percent-decoded key is exactly `token`,
  `access_token`, or `jwt`; other query pairs retain their order and encoded
  spelling.
- Fails closed or omits the whole ambiguous cookie header when it cannot safely
  prove that the `jwt` entry was removed.

Request-header transformations run before the final internal egress sanitizer,
so they cannot reintroduce a stripped credential or override a gateway-owned
internal field for a configured internal dispatch.

An upstream enables `internal_context` only after its deployed consumer requires
and validates the signed context. Signing or sanitization failure is fail
closed: Stargate sends no upstream request. Forwarding an original edge
credential together with the JWS is not supported by this protocol.

## Audience Scope and Multi-Hop Calls

An audience is a stable logical service identifier, for example
`urn:stargate:service:orders`. It is not a target URL, hostname, IP address,
pod name, router name, or environment-generated instance id.

The issuer MUST emit one audience string. The consumer MUST be configured with
that one exact accepted audience. Wildcards, suffix matching, substring
matching, multiple accepted audiences, and accepting any audience issued by
Stargate are forbidden in version 1.

A service MUST NOT relay an accepted token unchanged to another service. Its
audience and method/path binding are for the current hop. A later internal hop
needs a newly minted credential for its own exact audience and request binding;
version 1 does not define delegation or token exchange.

## Required Validation at the Receiving Service

A consumer adapter performs these checks before constructing a trusted context:

1. Reject a missing header, multiple `stargate-context` values, a malformed
   field value, or a value larger than 4,096 bytes.
2. Decode only enough protected-header data to select a key safely.
3. Require exact `typ`, pinned `RS256`, a bounded nonblank `kid`, and no
   token-directed key source or unsupported critical parameter.
4. Resolve `kid` only through the configured internal key/JWKS resolver.
5. Verify the JWS signature over the complete compact input.
6. Parse the payload using the strict version 1 schema and reject duplicate JSON
   member names, unknown fields, invalid UTF-8, and non-integer NumericDates.
7. Validate exact `iss`, exact `aud`, `iat`, `exp`, maximum lifetime, allowed
   clock skew, and uppercase ULID `jti`.
8. Validate the actor/authentication/sub/session/organization matrix.
9. Validate all field types, formats, numeric ranges, and byte limits.
10. Compare the raw request method and encoded path to the signed binding.
11. Require one valid `x-request-id` and compare it to `stg.request.id`.
12. Construct an immutable typed trusted context inaccessible through a general
    public deserializer.

Any failure rejects the internal request with a stable generic `401` response.
The response and logs MUST NOT reveal whether failure came from a key, issuer,
audience, signature, actor, time, or binding check. Internal bounded metrics may
record a low-cardinality reason category.

An unknown `kid` may trigger one bounded JWKS refresh. Fetch failure, stale
configuration, or unknown key after refresh never permits the token.

## Using Context for Authorization and Audit

After validation, a consumer may map:

- `actor.type` and `sub` to its trusted audit actor;
- `request.id`, `trace_id`, `client_ip`, and `user_agent` to audit request
  facts; and
- the optional organization id to an asserted organization.

The consumer MUST supply and validate:

- its own service identity;
- action and operation;
- resource type and immutable resource id;
- before/after snapshots and disclosure policy;
- outcome and safe metadata;
- event and operation ids; and
- the exact audit scope derived from the affected persisted resource and route
  boundary.

For organization scope, the consumer loads the affected resource's persisted
organization id and checks it against the signed assertion. Missing ownership,
missing assertion where one is required, or mismatch is an error. It never
falls back silently to application scope.

The business mutation and audit outbox insertion remain one local transaction.
The signed context is not stored as an audit event and the compact token is not
copied into audit metadata.

## Key Distribution and Rotation

- Version 1 keys are asymmetric RSA signing keys with at least 2,048 bits.
- The active private key is provisioned separately and never served by JWKS.
- The internal public JWK set is published separately from OAuth/OIDC JWKS.
- Rotation publishes the new public key alongside the retiring key before the
  issuer starts using the new `kid`.
- The retiring key remains published for at least maximum token lifetime,
  maximum accepted clock skew, and downstream cache overlap.
- Consumers refresh on unknown `kid` using bounded retry and retain a
  last-known-good cache, but never fail open.
- A missing or mismatched active key prevents a graph containing an
  `internal_context` block from becoming active.
- Production never silently generates replacement key material for a missing
  provisioned internal key.

The non-normative operational procedure is maintained in
[`internal-context-operations.md`](internal-context-operations.md).

## Security Considerations

### Protected assets

- Effective actor and authentication mechanism.
- Active organization and role assertion.
- Request and audit correlation facts.
- Internal signing private key.
- Edge access tokens, API keys, and session cookies.
- Availability of the gateway and internal services.

### Threats and required mitigations

| Threat | Attack / consequence | Required mitigation | Residual risk |
| --- | --- | --- | --- |
| Client header spoofing | Client supplies `stargate-context`, actor, org, or request id | Remove internal header and replace request id before routing; sign trusted facts; reserve internal header against transforms | Disabled/external upstreams receive no signed identity and must not infer one from other headers |
| Gateway bypass | Caller reaches a service directly without Stargate | Consumer requires a valid context; TLS/mTLS and network policy restrict ingress | Compromised network/service credentials remain an operational risk |
| Bearer replay | Captured JWS is replayed at the same service | TLS/mTLS, 30-second TTL, maximum 60 seconds, exact audience/method/path, unique JTI | Same endpoint replay inside the validity window remains unless optional JTI storage is later enabled |
| Audience substitution | Token for service A is presented to B | One explicit leaf audience and exact consumer comparison; no wildcards | Services deliberately sharing an audience share that isolation boundary |
| Algorithm confusion | Public token or attacker-selected HMAC is accepted | Exact type, pinned RS256, separate keys, reject `none`/`HS*`, mutually exclusive verifier | Signer-key compromise defeats issuer authenticity until rotation/revocation |
| Token-directed key injection | `jku`/`jwk` points verifier at attacker key | Reject token-directed key parameters; use configured resolver only | JWKS distribution channel still requires TLS and operational integrity |
| Edge credential leakage | Internal service receives replayable bearer/API-key/cookie/query credential | Final sanitizer on every configured HTTP/WS/replay dispatch | Upstreams without `internal_context` intentionally retain their separately governed forwarding behavior |
| Header/payload logging | Compact JWS reveals identity, IP, user agent and enables short replay | Redact compact and decoded token everywhere; strip response; use protected transport | Operators with packet/process access may still observe bearer material |
| Baggage injection/leakage | Client places PII or misleading identity in automatically propagated baggage | Do not forward baggage in version 1; never use it for auth | Disabled/external routes may keep independent existing policy |
| Path confusion | Different normalization makes a token valid for a different endpoint | Sign post-rewrite encoded path; compare raw encoded path before consumer routing; exclude query | Intermediary URI rewriting after Stargate must be prohibited or explicitly modeled |
| Clock manipulation/skew | Valid request rejected or replay window widened | NTP, bounded lifetime/skew, issue/expiry validation, skew metrics | Severe clock failure causes fail-closed availability impact |
| Organization confusion | Signed org is used for a resource owned elsewhere | Compare asserted org to persisted resource owner; no scope fallback | Incorrect persisted ownership remains a service data-integrity issue |
| Key compromise | Attacker mints arbitrary contexts | Least-access key storage, separate keys, rotation runbook, no private JWKS, incident response | All audiences signed by the compromised key are affected until rotation |
| Consumer compromise | Service steals/reuses received context | Audience/method/path/TTL limits and asymmetric verification-only keys | Compromised service can misuse contexts valid for its own audience during TTL |
| Hot-reload partial state | Graph emits contexts without the matching signer | Preflight signer/key/config before atomic graph swap; retain prior graph | External key file changes still require controlled operational rollout |
| Oversized/malformed input | Parser/crypto resource exhaustion or proxy rejection | 4 KiB pre-parse cap, strict limits, bounded JWKS refresh, parser rejection tests | High request volume still needs normal rate limiting |
| Mirror side effects | Shadow request duplicates a mutation | Signed `shadow` kind, isolated shadow target or service policy rejecting unsafe shadow writes | Marking alone does not prevent a service that ignores the field from writing |
| Trace spoofing | Caller manipulates trace ids to confuse telemetry/audit | Validate W3C format, sign active trace id, never authorize from trace context | External callers may legitimately originate trace ids; they remain diagnostic |

### Out of scope / accepted residual risks

- Version 1 does not provide JWE claim encryption.
- Version 1 does not store every accepted JTI for global replay prevention.
- Version 1 does not sign request bodies or arbitrary request headers.
- WebSocket message frames are not individually signed or reauthenticated.
- A trusted Stargate process with a compromised active private key can issue
  arbitrary contexts until the key is rotated and the incident contained.

## Compatibility and Versioning

- `stg.v = 1` selects this exact schema and validation profile.
- Version 1 parsers reject unknown fields rather than silently granting meaning
  to extensions.
- A future version uses a new `stg.v` and, when security profiles differ, a new
  explicit JOSE `typ` or mutually exclusive validation rule.
- Consumers may support multiple versions only with independent strict
  validators and explicit configuration.
- Absence of `internal_context` remains backward compatible and conveys no
  trusted anonymous identity.
- Presence of `internal_context` selects the complete version 1 issuance and
  credential-isolation behavior; there is no partial or migration mode.

## Version 1 Reference Summary

Version 1 uses the following fixed profile:

- Header name: `stargate-context`.
- Serialization: compact JWS with a JWT claims payload.
- JOSE profile: exact `typ`, RS256 only, separate key and internal JWKS.
- Lifetime: 30-second default, 60-second maximum.
- Size: 4,096-byte maximum compact token.
- Audience: one exact stable logical leaf-service audience.
- Activation: omit `internal_context` for pass-through behavior; include it
  with one exact audience for signed context and credential isolation.
- Baggage: not forwarded to version 1 internal upstreams.
- Dispatch: top-level `jti` is the sole dispatch id; request id is stable.
- Binding: exact HTTP method and post-rewrite encoded path, without query.
- Actors: user/JWT, API-key/API-key, and anonymous/none only.
- Organization: optional assertion; user role required, API-key role optional.
- Cookies: configured internal dispatches remove only Stargate's exact `jwt`
  cookie and preserve safely parsed unrelated cookies.
- Query credentials: configured internal dispatches remove `token`,
  `access_token`, and `jwt` after percent-decoding the key.
- Audit: propagate trusted inputs, never a completed business audit event.
- Mirrors: mark all mirror attempts `shadow`; service policy owns write safety.
- WebSockets: validate once at the upgrade handshake; frames carry no context.
- Delegation: an accepted token is never relayed unchanged to another audience.

Changes to any item in this summary require a new explicitly selected protocol
version. They do not alter the behavior of version 1 validators.

## Related Standards

- W3C Trace Context: <https://www.w3.org/TR/trace-context/>
- W3C Baggage: <https://www.w3.org/TR/baggage/>
- RFC 7515, JSON Web Signature: <https://www.rfc-editor.org/rfc/rfc7515>
- RFC 7519, JSON Web Token: <https://www.rfc-editor.org/rfc/rfc7519>
- RFC 8725, JSON Web Token Best Current Practices:
  <https://www.rfc-editor.org/rfc/rfc8725>
- RFC 9449, method/URI binding and replay design guidance:
  <https://www.rfc-editor.org/rfc/rfc9449>
- RFC 6648, deprecating new `X-` protocol fields:
  <https://www.rfc-editor.org/rfc/rfc6648>
