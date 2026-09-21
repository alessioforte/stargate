# Stargate - Project Context

## What It Is

IAM server + API gateway in Rust. Single binary, two deployment profiles:
- **Edge**: SQLite + in-memory store (single node)
- **Cluster**: PostgreSQL + Redis (horizontally scalable)

The gateway is under active development and now uses the explicit `stargate/v2alpha1` configuration model. Legacy gateway config without that schema is intentionally rejected.

## Workspace Structure

```
stargate/
├── src/               # Main binary
│   ├── main.rs        # Entry: init logging/DB/services, Axum router, Hyper server (plain or TLS)
│   ├── act/           # Stateful app flows (login guard, OTP/MFA, oauth state, signup, email verify, change password) + shutdown signal
│   ├── api/           # Axum routers + handlers + gateway fallback service
│   ├── aud/           # PostgreSQL-to-Redis audit outbox relay (cluster only)
│   ├── cli/           # Admin CLI (bootstrap super-admin)
│   ├── db/            # DB pool init (feature-gated postgres/sqlite)
│   ├── err/           # HTTP error types -> JSON responses (axum IntoResponse)
│   ├── etc/           # Config, CORS, GeoIP, JWT, TLS, guards, middleware, reqctx, proxy utils
│   └── fun/           # Business logic (token gen, name format, super-admin)
├── crates/
│   ├── ace/           # Access Control Engine (ABAC policy evaluation)
│   ├── ctx/           # Stargate-side signed internal context issuer and contract tests
│   ├── db/            # DB repo traits + sqlx implementations
│   ├── gate/          # Gateway config schema, compiler, runtime graph
│   ├── jwt/           # JWT encode/decode
│   ├── lb/            # Load balancing, health checks, circuit breaker core
│   ├── lim/           # Rate limiting + quota
│   ├── idp/           # External identity provider clients (Google, GitHub)
│   ├── oidc/          # Pure OAuth/OIDC provider helpers (PKCE, codes, refresh, claims, metadata)
│   ├── otp/           # Pure HOTP/TOTP + email/SMS OTP primitives
│   ├── pw/            # Passwords: Argon2 hashing (tunable params + pepper), policy engine, generator, API keys
│   ├── smtp/          # Email delivery (lettre)
│   ├── store/         # State store (DashMap / Redis)
│   └── tools/         # Shared utilities
├── apps/              # TypeScript frontends (see "Frontend (apps/)")
│   ├── console/       # Super-admin console (React + Vite + Mantine + Zustand)
│   ├── auth/          # Hosted login / auth-flow UI (same stack + layering)
│   └── mail/          # Transactional email templates (react-email)
├── migrations/        # sqlx migrations split by backend: postgres/, sqlite/
├── docs/              # Config docs, including config-v2alpha1.md
├── k8s/               # Kubernetes manifests
└── benches/           # Criterion benchmarks
```

## Key Architecture

### Stack
- **Runtime**: Tokio (multi-threaded)
- **HTTP server**: Axum 0.8 on Hyper 1 (via hyper-util auto builder)
- **Middleware**: tower + tower-http (cors, compression-gzip, trace, normalize-path)
- **WebSocket**: hyper-tungstenite + tokio-tungstenite for upstream proxying
- **TLS**: rustls + tokio-rustls + hyper-rustls (ring provider)
- **DB**: sqlx (feature-gated `postgres` or `sqlite`)
- **Auth**: JWT (jsonwebtoken), Argon2 passwords, RSA/HMAC signing
- **Serialization**: serde_json, serde_yaml_bw, MessagePack
- **OpenAPI**: utoipa + utoipa-axum + utoipa-swagger-ui
- **Observability**: tracing + tracing-subscriber + tracing-appender

### Gateway Runtime (`src/api/gateway/mod.rs`)

Mounted as Axum `fallback_service`. Current request flow:
1. Load active `Gate` from request extensions.
2. Pre-check API key/JWT and capture subject/auth kind.
3. Match the request against compiled v2 routers, sorted by descending priority.
4. Apply route middlewares: path rewrite, preserve host, request/response header transforms.
5. Apply selected policies in fixed runtime order: auth, access control, rate limit, quota.
6. Build an execution plan from the selected service: load-balanced upstream, weighted split, mirror, failover, or direct response.
7. Buffer replayable requests for mirror traffic and every multi-attempt failover plan.
8. Execute HTTP, WebSocket, or direct response and apply gateway/response headers.

Gateway modules:
- `routing.rs`: router and matcher evaluation.
- `middlewares.rs`: path and header transform application.
- `policies.rs`: auth, ACE, rate limit, quota policy execution.
- `planner.rs`: service graph expansion into an execution plan.
- `executor.rs`: upstream/direct-response execution, mirror dispatch, failover execution.
- `dispatch.rs`: shared internal-context sanitization, trace injection, and per-attempt signing boundary.
- `replay.rs`: request buffering and replay body limits.
- `headers.rs`, `path.rs`, `responses.rs`, `limits.rs`, `types.rs`: focused helpers and shared types.
- `http.rs`, `ws.rs`: protocol-specific proxy implementations.

### Gateway Config v2alpha1

`config.yaml` must contain:

```yaml
schema: stargate/v2alpha1
```

The config compiler uses named objects:
- `limits`: named rate/quota limit specs.
- `http.upstreams`: physical target pools.
- `http.services`: traffic actions that point to upstreams or compose other services.
- `http.middlewares`: reusable request/response transforms.
- `http.policies`: reusable auth/access/rate/quota checks.
- `http.routers`: ordered match rules that bind traffic to services, middlewares, and policies.

Named limits now use map form:

```yaml
limits:
  default:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 1s
```

Routers are sorted by descending `priority`; declaration order is preserved for ties.

Supported route matchers:
- Method
- Host / authority
- Path exact, prefix, template, regex
- Header values
- Query values
- Cookie values
- Source IP / CIDR
- Boolean combinators: `all`, `any`, `not`

Supported middleware:
- `strip_prefix`
- `add_prefix`
- `replace_path_regex`
- `preserve_host`
- `request_headers`
- `response_headers`

Supported service kinds:
- `load_balancer`
- `weighted`
- `mirror`
- `failover`
- `direct_response`

Supported policy kinds:
- `auth`
- `access_control`
- `rate_limit`
- `quota`

Auth strategies are `jwt` for native Stargate sessions, `api_key`, and
`oauth` for Authorization Code user access tokens. OAuth auth policies require
an exact `audience`; accepted tokens are bound back to the active `sid`, user,
enabled client, and session `client_ids` link before access-control evaluation.

Gateway rate limiting always falls back to the named `default` limit. A
non-default router `rate_limit` policy overrides that default for the matched
resource. Authenticated users/API keys can set `attrs.rate_limit` to a named
limit; that overrides the implicit default and an explicit `limit: default`, but
not a non-default resource policy.

Quota is independent from rate limiting. A router `quota` policy or subject
`attrs.quota` selects a named `quota_tracker` limit. If both rate limit and
quota are configured for a request, both checks run with separate tracker keys.

Full schema notes and examples: `docs/config-v2alpha1.md`. Internal-context
consumers follow `docs/internal-context-consumer-guide.md`; the protocol is
defined in `docs/internal-context-contract-v1.md`.

### Traffic Management

Implemented:
- Weighted traffic splitting between services.
- Shadow / mirror traffic with discarded mirror responses.
- Failover on transport error.
- Failover on selected response status codes.
- Direct response routes.
- Fallback routes via low-priority catch-all routers.
- Request replay buffering for mirrors and all multi-attempt failover plans.
- Replay body cap via `GATEWAY_REPLAY_BODY_LIMIT` (default `2MiB`).

Partially implemented:
- Circuit breaker is wired to both active liveness probes and live proxy traffic: transport errors and `502`/`503`/`504` responses trip it, mirror traffic does not. Per-upstream `fail_threshold`/`cooldown` are config-exposed via `load_balancer.circuit_breaker`. Breaker state is per-process (not shared across cluster nodes), and tripping on arbitrary response statuses is not configurable.
- HTTP/2 client support is enabled in the shared hyper client, but per-upstream protocol policy is not fully enforced.
- Upstream `transport.connect_timeout` exists. Read/idle/request timeout config is not complete.

Missing:
- Retry with backoff.
- Request hedging.
- gRPC proxying and gRPC-Web transcoding.
- GraphQL-aware routing and query-complexity limits.
- SSE-specific proxy handling as a first-class gateway feature.

### Audit (transactional outbox)

Audit producers construct and validate the versioned raw-event contract, then
store its final immutable JSON in `outbox_events` in the **same transaction** as
the mutation. `crates/db` service mutators receive a `TrustedAuditContext`, so
actor, request facts, and scope come from authenticated or persisted state. All
`/admin/*` events are `control_plane`. There is no in-process event buffer.

Super-admins can inspect delivery state through the read-only
`GET /admin/outbox-events` and `GET /admin/outbox-events/{event_id}` endpoints.
The list endpoint supports exact event and operation filters, pending or
published status, and limit/offset pagination. These reads never claim, lock,
publish, or otherwise mutate outbox rows.

- **Edge**: SQLite uses the aligned outbox schema, but starts no relay. New rows
  remain durable and pending with `published_at = NULL`.
- **Cluster**: one background relay per process (`src/aud/relay.rs`) ships
  PostgreSQL rows to Redis Streams:
  - Claims pending rows in `seq` order with `FOR UPDATE SKIP LOCKED`, allowing
    multiple nodes to drain disjoint batches.
  - Publishes to `AUDIT_REDIS_STREAM` (default `audit.raw`) with exactly one
    Redis field: `payload = <stored JSON>`.
  - Marks the claimed rows published and commits only after Redis accepts the
    batch. Publish-before-commit gives **at-least-once** delivery; consumers
    deduplicate using `event_id` inside the payload.
  - Redis/DB failures roll back the claim and use exponential backoff. Relay
    code never parses, rebuilds, substitutes, or logs the payload.

Login/logout audit events are not currently produced; any such coverage is a
separately reviewed follow-up rather than part of the core outbox contract.
The first admin session-registry API follows the same boundary: revocations
emit structured tracing but do not claim transactional outbox coverage across
the state-store/SQL boundary.

### Singletons / Runtime State

- Database pool (`DB`)
- Audit relay (cluster only)
- JWT config
- GeoIP database
- Gateway runtime config and compiled HTTP graph
- Gateway load balancers
- ACE policy engine
- Limiter/quota service

Each login session uses the standard JWT `sid` as its stable identifier. Native
access and refresh tokens retain the same `sid` for the lifetime of the login,
and linked user OAuth/OIDC tokens carry it as well. Native refresh-token replay
protection is independent: each refresh token has a rotating `jti`, whose
current value is stored under `account:session-refresh:{sid}` and updated
atomically.

The state store keeps a per-user session index plus the global
`account:sessions:directory` hash. Directory fields expire with the native
refresh-token TTL and hold the stable sid, org, authentication/last-seen/expiry
times, and linked OAuth client IDs. Removing a session deletes its sid-backed
subject and refresh-rotation state; Stargate's admin, userinfo, introspection,
and OAuth-refresh paths also require linked user OAuth tokens to have a live
backing session. OAuth tokens validated offline by an external resource server
remain subject to their normal JWT expiry.

### Config Hot-Reload

`notify` watches `config.yaml` and policy files. Content hash prevents false positives.

Hot reload behavior:
- Parse and compile the new v2 config first.
- If valid, atomically swap the active gateway graph/balancers/policies.
- If invalid, log the error and keep the previous in-memory config active.

Admin config endpoints validate v2 config before saving.

Admin access-control endpoints manage the ACE rule file (`.stargate/policies`)
separately from `config.yaml`:
- `GET /admin/access-control/rules` returns raw content, parsed rules,
  diagnostics, and a `sha256:*` revision.
- `PUT /admin/access-control/rules` requires the current revision, validates the
  whole document strictly, atomically replaces the file, and reloads the live ACE
  engine immediately.
- `POST /admin/access-control/rules/validate` validates proposed rule content
  without saving.
- `POST /admin/access-control/rules/evaluate` evaluates the active in-memory
  policy snapshot against a supplied subject/resource/action/context.
- `POST /admin/access-control/capabilities/evaluate` enumerates resource/action
  capabilities declared in that snapshot for supplied actor attributes,
  organization context, and environment. It does not inspect gateway routes.

The gateway and both admin evaluation endpoints share one atomically swapped
snapshot containing the `PolicyEngine` and its `sha256:*` revision. Evaluation
requests do not read the policy file. Reloads strictly parse the complete
document and keep the prior snapshot active on failure.

ACE rule metadata can be stored in comments immediately above a rule, for
example `# @id reports-read-admin`; the ACE parser ignores these comments.

### Feature Flags

```toml
edge    = ["sqlite", "memory"]
cluster = ["postgres", "redis"]
```

Compile-time backend selection. No runtime backend branching.

## OAuth/OIDC Provider

Stargate is now also an OAuth 2.0 authorization server and OpenID Connect provider with admin-managed clients only.

Source layout:
- `crates/oidc`: pure provider logic with no Axum or DB driver dependency.
- `src/api/oidc`: Axum bindings for provider endpoints and well-known metadata.
- `src/api/social`: Google/GitHub OAuth consumer login callbacks and state endpoint.
- `src/api/admin/oauth_clients.rs`: admin-managed OAuth client CRUD.

Implemented provider endpoints:
- `/.well-known/oauth-authorization-server`
- `/.well-known/openid-configuration`
- `/.well-known/jwks.json`
- `/oauth/authorize`
- `/oauth/token`
- `/oauth/userinfo`
- `/oauth/introspect`
- `/oauth/revoke`
- `/admin/oauth/clients/*`

Supported grants and flows:
- `client_credentials`
- Authorization Code + PKCE (`S256`)
- `refresh_token` with opaque rotating refresh-token families

Supported OIDC scopes/claims:
- `openid`: `sub`
- `email`: `email`, `email_verified`
- `profile`: `name`, `preferred_username`, `picture`
- `offline_access`: enables refresh-token issuance when client allows `refresh_token`

Provider state:
- OAuth clients live in SQL table `oauth_clients`.
- Consent records live in SQL table `oauth_consents`.
- Authorization codes live in `store` under `oauth:authorization-code:{code_hash}` with TTL.
- Refresh-token families live in `store` under `oauth:refresh-family:{family_id}` with TTL and compare-and-swap rotation.
- JWT `jti` revocation state lives in `store` until token expiry.

Client notes:
- Dynamic client registration is not supported.
- Client secrets are returned only on create/rotate and stored only as hashes.
- Public clients use `token_endpoint_auth_method = none` and PKCE.
- First-party clients can skip consent with `attrs.first_party = true`, `attrs.firstParty = true`, or `attrs.trusted = true`.
- Privileged token operation clients can use `oauth:introspect` / `oauth:revoke` scopes or `attrs.can_introspect` / `attrs.can_revoke`.

Full usage guide: `docs/oauth-oidc-provider-guide.md`.

## OTP, Passwordless Login, and MFA

Stargate includes a pure OTP crate plus account-level email OTP and MFA flows.

Source layout:
- `crates/otp`: dependency-light OTP primitives. Implements HOTP (RFC 4226), TOTP (RFC 6238), Base32, authenticator-app provisioning URIs, and short-lived message OTP records for email/SMS.
- `src/act/otp`: stateful OTP/MFA application services. Owns policy evaluation, authenticated-user extraction, OTP challenge persistence, verification, delivery, request throttling, and service-level flows.
- `src/api/account/otp`: thin Axum bindings and OpenAPI annotations for account OTP/MFA endpoints.
- `src/etc/env.rs`, `src/etc/input.rs`, `src/etc/time.rs`: small shared helpers used by OTP and available for other modules.

Implemented account OTP endpoints:
- `POST /account/login/otp/email` - start passwordless email OTP login. Response is generic for unknown users.
- `PUT /account/login/otp/email` - verify passwordless email OTP and issue a normal session when MFA policy allows passwordless login.
- `PUT /account/login/mfa/challenges/{challenge_id}` - verify the pending MFA challenge created by password login and issue a normal session.
- `GET /account/mfa/methods` - list effective MFA policy/methods for the authenticated user.
- `POST /account/mfa/challenges` - create an authenticated MFA challenge, currently email only.
- `PUT /account/mfa/challenges/{challenge_id}` - verify an authenticated MFA challenge and store a short-lived step-up marker.

Login behavior:
- Normal password login still verifies username/password first.
- If effective MFA policy requires MFA, `POST /account/login` returns `202` with `mfaRequired`, `challengeId`, method, expiry, TTL, and max-attempt metadata instead of issuing tokens.
- The client then calls `PUT /account/login/mfa/challenges/{challenge_id}` with `{ "code": "123456" }` to receive the normal access/refresh token response.
- Passwordless email OTP is disabled for users whose effective MFA policy requires MFA, because email OTP alone would otherwise become a single-factor downgrade.

MFA policy:
- Global mode comes from `MFA_MODE`: `off`, `optional`, or `required`.
- In `optional`, users opt in with `users.attrs.mfa.enabled = true`. Super-admins also require MFA when `MFA_REQUIRED_FOR_SUPER_ADMIN=true`.
- User attrs can restrict/prefer methods:

```json
{
  "mfa": {
    "enabled": true,
    "methods": ["email"],
    "preferredMethod": "email"
  }
}
```

Admin step-up:
- `MFA_ADMIN_STEP_UP_REQUIRED=true` makes super-admin user sessions require a valid step-up marker with purpose `admin` before admin grants are added.
- Admin API keys are not affected by user-session MFA step-up.
- Step-up flow: `POST /account/mfa/challenges` with `{ "method": "email", "purpose": "admin" }`, then `PUT /account/mfa/challenges/{challenge_id}`.
- Step-up marker TTL is controlled by `MFA_STEP_UP_TTL_SECS`.

OTP state:
- Passwordless email challenge: `account:login:otp:email:{challenge_id}`
- Pending login MFA challenge: `account:mfa:pending-login:{challenge_id}`
- Authenticated MFA challenge: `account:mfa:challenge:{user_id}:{challenge_id}`
- Step-up marker: `account:mfa:verified:{sid}:{purpose}`
- OTP request throttling counters live under `account:otp:request:*`.

Delivery notes:
- Email OTP uses `smtp::Smtp` with custom subject/body support.
- Blocking SMTP send calls are isolated with `tokio::task::spawn_blocking`.
- Passwordless request delivery is backgrounded to reduce account-enumeration timing differences.

Currently implemented methods:
- Email OTP for passwordless login, login MFA, and authenticated step-up MFA.
- HOTP/TOTP primitives exist in `crates/otp`, but authenticator-app enrollment/verification APIs are not yet wired into account MFA.
- SMS OTP primitives exist in `crates/otp`, but SMS delivery/account APIs are not yet wired.

## Database Schema (migrations/)

Tables include `organizations`, `users`, `super_admins`, `credentials`,
`credential_history`, `api_keys`, `admin_keys`, `service_accounts`,
`oauth_clients`, `oauth_consents`, `user_organizations`, `user_api_keys`,
`service_account_api_keys`, and `outbox_events`.

Entity and audit event ids are ULIDs stored as text. `outbox_events` stores the
immutable raw JSON payload plus local `seq`, optional `operation_id` correlation
metadata, `created_at`, and nullable `published_at`. PostgreSQL and SQLite expose
the same logical outbox columns.

## Key Env Vars

| Var | Default | Purpose |
|-----|---------|---------|
| `PORT` | 5050 | HTTP listen port |
| `STARGATE_RUNTIME_PROFILE` | - | `edge` or `cluster` |
| `JWT_ALGORITHM` | `RS256` | JWT signing algorithm |
| `JWT_SECRET` | - | HMAC signing key for HS* algorithms |
| `JWT_ISSUER` | `http://localhost:$PORT` | JWT issuer and OAuth/OIDC issuer |
| `JWT_KID` | `stargate-current` | JWT key id |
| `JWT_ACCESS_EXP` | 1h | Access token TTL |
| `JWT_REFRESH_EXP` | 1d | Refresh token TTL |
| `OAUTH_BASE_URL` | issuer/localhost | Public base URL for metadata endpoint links |
| `JWKS_CACHE_MAX_AGE_SECS` | 300 | JWKS cache max-age |
| `INTERNAL_CONTEXT_ALGORITHM` | RS256 | Internal-context signing algorithm; version 1 accepts only RS256 |
| `INTERNAL_CONTEXT_ISSUER` | - | Exact issuer for signed internal request contexts |
| `INTERNAL_CONTEXT_KID` | - | Active internal-context signing key id |
| `INTERNAL_CONTEXT_PRIVATE_KEY_PATH` | - | Dedicated internal-context RSA private key |
| `INTERNAL_CONTEXT_JWKS_PATH` | - | Dedicated public internal-context JWKS |
| `INTERNAL_CONTEXT_TTL_SECS` | 30 | Internal-context token lifetime |
| `INTERNAL_CONTEXT_CLOCK_SKEW_SECS` | 5 | Consumer clock-skew allowance |
| `INTERNAL_CONTEXT_JWKS_CACHE_MAX_AGE_SECS` | 60 | Published internal JWKS cache max-age |
| `POSTGRES_ENDPOINT` | - | Postgres host (cluster) |
| `POSTGRES_USERNAME` | - | Postgres user |
| `POSTGRES_PASSWORD` | - | Postgres password |
| `POSTGRES_DATABASE` | - | Postgres DB name |
| `REDIS_URL` | - | Redis (cluster) |
| `RUST_LOG` | - | Log filter |
| `LOG_DIR` | - | Log file dir |
| `OTEL_ENABLED` | auto | Enables OTLP traces, metrics, and logs; defaults true when an OTLP endpoint is set |
| `OTEL_SERVICE_NAME` | stargate | OpenTelemetry service name |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | - | Shared OTLP endpoint for traces, metrics, and logs |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | grpc | OTLP transport protocol (`grpc` or `http/protobuf`) |
| `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` | - | Trace-specific OTLP endpoint override |
| `OTEL_EXPORTER_OTLP_METRICS_ENDPOINT` | - | Metric-specific OTLP endpoint override |
| `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT` | - | Log-specific OTLP endpoint override |
| `AUDIT_RELAY_ENABLED` | true | Run the audit outbox relay (cluster) |
| `AUDIT_RELAY_BATCH` | 256 | Rows claimed per relay batch |
| `AUDIT_RELAY_INTERVAL_MS` | 1000 | Idle poll interval when caught up |
| `AUDIT_REDIS_STREAM` | audit.raw | Redis stream receiving one unchanged `payload` field per event |
| `AUDIT_STREAM_MAXLEN` | 100000 | Approx stream cap (`MAXLEN ~`; 0 = unbounded) |
| `TLS_ENABLED` | false | HTTPS |
| `TRUSTED_PROXIES` | - | IP/CIDR for client IP |
| `CORS_ORIGINS` | - | Allowed origins |
| `TRUSTED_ORIGINS` | - | Trusted browser origins for sensitive IAM endpoints |
| `GEOIP_DB_PATH` | - | MaxMind GeoLite2 |
| `GATEWAY_REPLAY_BODY_LIMIT` | 2MiB | Max buffered body for mirror/failover replay |
| `SERVER_DRAIN_DELAY_SECS` | 5 | Time with readiness disabled before listeners close; 0 disables the delay |
| `SERVER_SHUTDOWN_TIMEOUT_SECS` | 25 | TLS connection drain deadline after listeners close |
| `EMAIL_OTP_PEPPER` | - | Server-side HMAC pepper for email/message OTP records |
| `EMAIL_OTP_LENGTH` | 6 | Email OTP code length |
| `EMAIL_OTP_TTL_SECS` | 300 | Email OTP challenge TTL |
| `EMAIL_OTP_MAX_ATTEMPTS` | 5 | Max invalid verification attempts per OTP challenge |
| `MFA_MODE` | optional | MFA mode: `off`, `optional`, or `required` |
| `MFA_METHODS` | email | Enabled MFA methods; only `email` is wired today |
| `MFA_DEFAULT_METHOD` | email | Default MFA method when user attrs do not specify one |
| `MFA_EMAIL_ENABLED` | true | Enables email MFA challenges |
| `MFA_REQUIRED_FOR_SUPER_ADMIN` | true | Requires MFA for super-admin users when `MFA_MODE=optional` |
| `MFA_ADMIN_STEP_UP_REQUIRED` | false | Requires an authenticated `admin` MFA step-up marker for super-admin user-session admin grants |
| `MFA_STEP_UP_TTL_SECS` | 300 | TTL for authenticated MFA step-up markers |
| `CONSOLE_OAUTH_CLIENT_ID` | stargate_console | Reserved public OAuth client provisioned for the Console |
| `CONSOLE_OAUTH_REDIRECT_URI` | derived | Exact Console callback; falls back through `CONSOLE_PUBLIC_URL`, `OAUTH_BASE_URL`, or `JWT_ISSUER` |
| `CONSOLE_PUBLIC_URL` | - | Public Console origin used to derive the bootstrap callback |
| `PASSWORD_POLICY_*` | see `.env.example` | Global password policy (length, classes, banned list, expiry, history); per-org override via `organizations.attrs.password_policy`. Guide: `docs/password-policies-guide.md` |
| `ARGON2_MEMORY_KIB` | 19456 | Argon2id memory cost; hash upgrades apply on next login |
| `ARGON2_ITERATIONS` | 2 | Argon2id time cost |
| `ARGON2_PARALLELISM` | 1 | Argon2id lanes |
| `PASSWORD_PEPPER` | - | Optional Argon2 secret; pre-pepper hashes verify and rehash on login |

Full list: `.env.example`

## Build & Run

```bash
# Dev (defaults to edge/sqlite)
cargo run

# Edge release
cargo build --release --features edge

# Cluster release
cargo build --release --features cluster

# Docker
docker build --build-arg STARGATE_PROFILE=edge -t stargate:edge .
docker build --build-arg STARGATE_PROFILE=cluster -t stargate:cluster .
```

## CLI: Bootstrap Instance

```bash
cargo run -- admin bootstrap \
  --email admin@example.com \
  --generate-password \
  --name "Admin User" \
  --nickname admin \
  --oauth-redirect-uri https://identity.example.com/stargate/auth/callback
```

The command atomically creates the first super-admin and the reserved Console
OAuth client. It is idempotent when both resources match. If the administrator
already exists but the client is missing, omit the credential/profile flags and
rerun with the OAuth settings.

Flags: `--password <val>`, `--password-stdin`, `--generate-password`,
`--oauth-client-id`, `--oauth-redirect-uri`

## CLI: Generate Internal-Context Key Material

```bash
cargo run --features edge -- internal-context generate-key \
  --output-dir .stargate/internal-context \
  --kid stargate-internal-current
```

This explicit provisioning command creates `private.pem` and a matching
public-only `jwks.json`. It refuses to overwrite existing files. Stargate
server startup validates but never generates missing internal-context keys.

## API Overview

- `GET /livez` - dependency-free process liveness; also suitable for startup probes
- `GET /readyz` - traffic readiness: initialized, not draining, SQL and (cluster) Redis available
- `GET /health` - readiness compatibility endpoint preserving name/version/healthy status fields
- `GET /docs` - Swagger UI
- `GET /docs/errors` - machine-readable error catalog used by documentation UIs
- `GET /i18n/{locale}` - versioned API error/success message catalog (`en`, `it`)
- `/.well-known/*` - OAuth/OIDC metadata and public JWKS
- `GET /.well-known/stargate-context-jwks.json` - dedicated public
  internal-context JWKS when the signer is configured
- `/account/*` - login, logout, profile, account refresh tokens, credentials
- `/account/login/otp/email`, `/account/login/mfa/challenges/*`, `/account/mfa/*` - passwordless email OTP and MFA
- `/oauth/authorize`, `/oauth/token`, `/oauth/userinfo`, `/oauth/introspect`, `/oauth/revoke` - OAuth/OIDC provider endpoints
- `/oauth/state`, `/oauth/github`, `/oauth/google` - Google/GitHub consumer login endpoints
- `/signup/*` - registration + email verification
- `GET /admin/me` - validated user/admin-key principal, authentication details, token timing, scopes, and canonical effective permissions
- `GET /admin/overview` - aggregate resource and active-session counts, audit delivery backlog, and active gateway graph counts
- `GET /admin/sessions` - paginated active-session registry, filterable by user, organization, and OAuth client
- `DELETE /admin/sessions/{session_id}` - revoke one login session by its standard JWT `sid`
- `DELETE /admin/users/{user_id}/sessions` - revoke all login sessions for a user
- `GET /admin/health` - dependency health with runtime profile, backend kinds, latency, and check timestamp
- `POST /admin/users` and `POST /admin/users/invitations` - accept an optional
  `membership: { organizationId, role? }`; the admin key remains
  application-wide and may target any organization
- `/admin/*` - super-admin users bypass capability checks; admin keys use exact
  `<resource>:<action>` permissions from `src/api/admin/authorization.rs`

## Error Response Format

Errors are JSON responses with request ID in the `x-request-id` response header.

Typical body shape:

```json
{
  "message": "User not found",
  "code": "user.not_found",
  "type": "not_found",
  "link": "https://docs.example.com/errors/user.not_found",
  "params": {
    "id": "01J..."
  }
}
```

`code` is the stable, domain-specific API contract and the i18n lookup key.
`message` is the English fallback, while `params` contains safe interpolation
values for localized clients. `type` is the broad error category; it must not
be used in place of `code` for application behavior. `link` points to the
future error documentation page for that exact code.

The English catalog is generated from the Rust error/message definitions.
Locale files contain overrides and fall back to English, so every published
catalog always contains every code. Catalog responses include `ETag`,
`Content-Language`, a short cache lifetime, and a content-derived `version`.
OAuth protocol error responses retain their RFC-defined envelope where
required.

HTTP codes currently used include: 400, 401, 403, 404, 409, 413, 429, 500, 502, 503.

## Rate Limit Response Headers

`x-ratelimit-limit`, `x-ratelimit-remaining`, `x-quota-limit`, `x-quota-remaining`, `retry-after`

## Graceful Shutdown

SIGTERM/SIGINT -> disable readiness -> wait `SERVER_DRAIN_DELAY_SECS` (5s) ->
close listeners and drain requests -> stop audit relay -> save in-memory state.
TLS draining has a 25s default deadline (`SERVER_SHUTDOWN_TIMEOUT_SECS`);
plain HTTP uses Axum graceful shutdown without an application drain deadline.

Probe routes are mounted outside authentication and rate limiting. `/livez`
stays successful while the listener responds, including the shutdown delay.
Readiness and `/admin/health` share dependency checks in `src/etc/health.rs`,
with concurrent one-second timeouts. Invalid config reloads keep the active
graph and do not disable readiness; upstreams, SMTP, identity providers,
telemetry, and audit delivery lag are not readiness dependencies.
See `docs/health-probes.md` for contracts and deployment guidance.

## Frontend (apps/)

`apps/` holds the TypeScript frontends, separate from the Rust binary. Three apps:

- `console` — super-admin console (users, orgs, API/admin keys, OAuth clients, service accounts, gateway config, access-control policies). This is the reference implementation of the layering below.
- `auth` — hosted login / auth-flow UI (password login, passwordless email OTP, login MFA, reset/change password). Same stack, same layering (`services/`, `store/`, `app/routes/`, `components/`).
- `mail` — transactional email templates (`react-email`). A build-time template project, **not** a runtime SPA; it does not follow the layering below.

Stack (console/auth): React 19, Vite, TypeScript, Mantine v9 (`@mantine/core|form|hooks|notifications`), Zustand v5, react-router v7, axios, `@tanstack/react-table`, react-icons. Path alias `@/*` maps to the app root, so imports read `@/services`, `@/store`, `@/components`, `@/i18n`, `@/lib`. Build-time API base URL comes from `VITE_API_URL`.

Dev commands (run inside `apps/console` or `apps/auth`): `npm run dev` (Vite), `npm run build` (`tsc -b && vite build`), `npm run lint` (oxlint), `npm run format` (prettier).

The console home uses `/admin/me` as its authentication probe so a dependency
outage does not block the console shell. It polls `/admin/health` separately
and loads `/admin/overview` plus recent outbox events for the operational
dashboard.

### Layered Architecture

The single most important convention is a strict, **one-way** data flow. Each layer may only depend on the layer to its right:

```
components/          app/routes/ (pages)      store/                services/
(agnostic UI)   <—   (render + wiring)   <—   (state + actions)  <—  (raw API)
```

- Only `store/` imports `services/`.
- Only `app/routes/` (pages) import `store/`.
- `components/` import **neither** the store nor services — they are driven entirely by props.
- Types flow the other way: the DTOs in `services/types.ts` are the shared contract imported by every layer.

Keep each concern in exactly one layer. Data-shaping, the loading/error lifecycle, notifications, and "what happens next" live in the store; pages only wire; components only render.

#### 1. `services/` — raw API (no React, no state)

- `http.ts`: the `Http` class over axios. `authRequest<T>()` attaches bearer auth + `X-Org-Context` and does proactive/reactive token refresh via `TokenManager`; `request<T>()` is for unauthenticated calls. Every call returns a uniform envelope `Response<T> = { data, error?, message?, status?, headers? }` — HTTP/error outcomes are captured into the envelope, never thrown.
- `api.ts`: `AdminApiService` — one thin method per backend endpoint (`getOrganizations`, `createOrganization`, `updateOrganization`, …), each just building an `AxiosRequestConfig` and returning `Response<T>`. No state, no side effects beyond the HTTP call.
- `index.ts`: a lazy `Service` singleton (via a `Proxy`) that wires `Http` + `TokenManager` + `AdminApiService`, reads runtime config, and owns token storage. Exported as `default services`.
- `types.ts`: request/response DTOs (the shared contract). `token-manager.ts`, `jwt.ts`: token lifecycle + JWT decode helpers.

Rule: services speak HTTP only. They never import React or the store.

#### 2. `store/` — Zustand: state + actions (the only caller of services)

- `index.ts`: one store, created with `create()` + the `devtools` middleware over a `store: StateCreator<State & Actions>`. `initialState` holds each entity as a `StoreItem`.
- `types.ts`: the `State` (fields) and `Actions` (method signatures) interfaces — the store contract.
- `item.ts`: `StoreItem<Data>` wraps every async slice as `{ data, meta, status, message }` with fluent transitions `setLoading()` / `setError(msg)` / `setSuccess(data)` and guards `isLoading()` / `isError()` / `isSuccess()` / `isIdle()`. This is the canonical async-state shape used across the store.
- `settings.ts`: localStorage-backed prefs (theme, language). `accessor.ts`: dot-path `get`/`set`/`delete` for nested objects.

Actions follow one shape — flip the item to loading, call the service, branch on the envelope; mutations also raise a notification and re-fetch:

```ts
createOrganization: async (organization) => {
  const organizations = get().organizations;
  set({ organizations: organizations.setLoading() });

  const { error, message } = await Service.admin.createOrganization(organization);
  if (error) {
    set({ organizations: organizations.setError(message) });
    showNotification({ type: "error", title: "Error", message });
    return;
  }

  showNotification({ type: "success", title: "Success", message: "Organization created successfully" });
  get().getOrganizations(); // refresh the list
},
```

Rule: actions own the loading/error lifecycle, notifications, and session/token orchestration. Components subscribe to state and dispatch actions; they never call services directly.

#### 3. `app/routes/` — React pages (render + action wiring)

- `app.tsx`: the `createBrowserRouter` table; each path renders a page, wrapped by `Layout` (the authed shell).
- A page (e.g. `organizations/organizations.tsx`): pulls state + actions from `useStore()`, loads on mount with `useEffect(() => getOrganizations(), …)`, holds only local UI state (`useDisclosure` for a drawer, the selected row), and composes agnostic components — passing store actions down as callbacks (`onSave={createOrganization}`). Labels come from `useTranslations()` (`@/i18n`).
- Co-located, page-specific files: `columns.tsx` (table column defs), `create-*.tsx` / `edit-*.tsx` (drawer forms built with `@mantine/form` that assemble the request DTO and hand it to an action), `options.ts` / `*-utils.ts` (route-local helpers). These are intentionally **not** shared.

Rule: pages are glue. They know the store and the domain DTOs and wire them into components; they don't call services and don't hold canonical data.

#### 4. `components/` — agnostic, reusable UI

- Barrel `components/index.ts` re-exports everything (`Table`, `EntityDrawer`, `Shell`, `MultiDrawer`, `CodeBox`, `Icon`, `Loader`, `JsonAttributesForm`, `showNotification`, `useConfirmModal`, …).
- Driven purely by props/children, with **no** store, services, or domain-type imports — e.g. `EntityDrawer({ opened, title, onClose, children })`, `Table({ columns, data, loading, meta })`. The same `Table` / `EntityDrawer` serve every entity.

Rule: if a component needs `@/store` or `@/services`, it belongs in `app/routes/`, not here.

### End-to-end example (add a field to Organizations)

1. `services/types.ts` — extend the DTO (`CreateOrganizationRequest`).
2. `services/api.ts` — no change unless the endpoint/path/verb changes.
3. `store/types.ts` + `store/index.ts` — adjust the action signature only if new; the existing action already forwards the DTO.
4. `app/routes/organizations/create-organization.tsx` — add the form field and include it in the built payload.
5. `components/` — touch only if a new reusable primitive is required.

Following the arrows keeps each concern in exactly one place.

## Current Notes

- Request ID: ULID per request, in response header `x-request-id`, in audit logs.
- Thread-local LRU caches for ACE decisions are version-aware and invalidated on policy change.
- WebSocket proxying is supported in the gateway via `hyper-tungstenite`.
- HTTP proxying strips hop-by-hop headers and supports preserve-host behavior.
- GeoIP is available via MaxMind GeoLite2-City, but geo-aware routing is not yet first-class in v2 matchers.
- OpenAPI docs use `utoipa` with `utoipa-axum` + `utoipa-swagger-ui`.
- `api::server_router` applies rate limiting to IAM/admin APIs and merges probe routes outside that layer; `main.rs` adds security headers, tracing, CORS, compression, and normalize-path.
- TLS server uses a custom hyper-util connection loop with per-connection graceful shutdown and drain deadline.
- OAuth/OIDC browser consent UI is not implemented; third-party clients without stored consent receive `consent_required`.
- OAuth/OIDC non-redirect errors still use Stargate's generic error envelope rather than full RFC-shaped error bodies.
- OTP/MFA API modules are intentionally thin Axum/OpenAPI bindings; workflow changes should usually go in `src/act/otp`.
- Email OTP is currently the only wired account MFA method. TOTP/HOTP and SMS primitives exist in `crates/otp` for future account integrations.
- Internal-context issuance exports bounded outcome, signing-duration, and
  token-size telemetry. Operations, rotation, recovery, alerts, and the local
  signing benchmark are documented in `docs/internal-context-operations.md`.
