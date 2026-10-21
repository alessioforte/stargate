# Stargate - Project Context

## What It Is

IAM server + API gateway + OAuth 2.0 / OpenID Connect provider in Rust. Single
binary, two deployment profiles:
- **Edge**: SQLite + in-memory store (single node)
- **Cluster**: PostgreSQL + Redis (horizontally scalable)

The gateway uses the `stargate/v1` configuration model. Supplied YAML and admin
JSON must declare `schema: stargate/v1`; other identifiers are rejected.

This file is the single source of project context for coding agents.
`.claude/CLAUDE.md` (gitignored) only imports it; update this file when the
architecture changes.

## Workspace Structure

```
stargate/
├── src/               # Main binary
│   ├── main.rs        # Entry: init logging/telemetry/DB/services, Axum router, Hyper server (plain or TLS)
│   ├── act/           # Stateful app flows: login guard, OTP/MFA, sessions, orgs, oauth state, signup,
│   │                  #   email verify, change password, password policy, token revocation, ACE rules
│   ├── api/           # Axum routers + handlers; gateway fallback service; auth/console app hosting
│   ├── aud/           # PostgreSQL -> Redis audit outbox relay (cluster only)
│   ├── cli/           # CLI: admin bootstrap, internal-context / jwt generate-key
│   ├── db/            # DB pool init (feature-gated postgres/sqlite)
│   ├── err/           # Error catalog (ErrorCode) -> JSON responses; OAuth error envelope
│   ├── etc/           # Shared support: auth, http, request_context, observability, server, gate,
│   │                  #   internal_context, store, env/input/limits/time helpers
│   └── fun/           # Small helpers (token gen, cookies, base/auth-app URLs, shutdown signal, super-admin)
├── crates/
│   ├── ace/           # Access Control Engine (ABAC policy evaluation)
│   ├── ctx/           # Signed internal-context (`stargate-context` header) contract, issuer, verifier
│   ├── db/            # DB repo traits + sqlx implementations, TrustedAuditContext, outbox
│   ├── gate/          # Gateway config schema (cfg/v1.rs), compiler, runtime graph
│   ├── idp/           # External identity provider clients (Google, GitHub)
│   ├── jwt/           # JWT encode/decode/validation (typed tokens), keyring (`kid`-selected keys)
│   ├── lb/            # Load balancing, health checks, circuit breaker core
│   ├── lim/           # Rate limiting + quota
│   ├── oidc/          # Pure OAuth/OIDC provider helpers (PKCE, codes, refresh, claims, metadata)
│   ├── otp/           # Pure HOTP/TOTP + email/SMS OTP primitives
│   ├── pw/            # Passwords: Argon2 (tunable params + pepper), policy engine, generator, API keys
│   ├── smtp/          # Email delivery (lettre)
│   ├── store/         # State store (DashMap / Redis)
│   └── tools/         # Shared utilities
├── apps/              # TypeScript frontends (see "Frontend")
│   ├── auth/          # Hosted login / auth-flow UI, served at /auth
│   ├── console/       # Super-admin console, served at /stargate
│   └── mail/          # Transactional email templates (react-email, build-time only)
├── migrations/        # sqlx migrations split by backend: postgres/, sqlite/
├── docs/              # Guides: config-v1, oauth-oidc-provider, oidc-conformance, multi-org,
│                      #   password-policies, health-probes, internal-context-*, cli, docker, api errors
├── k8s/               # Kubernetes manifests
└── benches/           # Criterion benchmarks; scripts/ has gateway/backend benchmark runners
```

## Key Architecture

### Stack
- **Runtime**: Tokio (multi-threaded)
- **HTTP server**: Axum 0.8 on Hyper 1 (hyper-util), one tracked connection loop for plain HTTP and TLS
- **Middleware**: tower + tower-http (cors, compression-gzip, trace, normalize-path)
- **WebSocket**: hyper-tungstenite + tokio-tungstenite for upstream proxying
- **TLS**: rustls + tokio-rustls + hyper-rustls (ring provider)
- **DB**: sqlx (feature-gated `postgres` or `sqlite`)
- **Auth**: JWT (jsonwebtoken), Argon2 passwords, RSA/HMAC signing
- **Serialization**: serde_json, serde_yaml_bw, MessagePack
- **OpenAPI**: utoipa + utoipa-axum + utoipa-swagger-ui
- **Observability**: tracing + OpenTelemetry (OTLP traces, metrics, logs)

### Shared Support (`src/etc/`)

- `auth/`: access control, subjects, session JWT config, password helpers.
  `identity.rs` owns verified facts; `verification.rs` owns credential/backing-state checks.
- `http/`: request extraction, trusted proxies, origins/CORS (`origin.rs` trusted-origin
  middleware), security and retry headers, IAM limits (`rate_limit.rs`), message envelopes.
- `request_context/`: request metadata, ingress headers, lazy GeoIP; signed-context
  drafts (`propagation.rs`) and audit/authorization projections (`adapters.rs`).
- `observability/`: logging and the telemetry provider (`telemetry/metrics.rs`,
  `telemetry/propagation.rs`).
- `server/`: listeners, listener TLS, health lifecycle/probes (`health.rs`),
  profile validation, startup banner.
- `gate/`: gateway init, `config.rs` preparation/files, `runtime.rs` atomic snapshot
  activation, `reload.rs` watchers, `resources.rs` process admission/memory permits/
  tracked tasks, `transport/` prepared outgoing HTTP/TLS/mTLS clients.
- `internal_context/`: signer runtime, `settings.rs`, `material.rs` key/JWKS validation,
  `signing.rs` bounded CPU workers.
- `store/`: selected profile interface; `memory.rs` persistence, `redis.rs` init/readiness.

### Gateway Runtime (`src/api/gateway/`)

Mounted as the Axum `fallback_service`. Request flow:
1. Pin the active `RuntimeSnapshot` (graph, balancers, limiter, prepared clients,
   version) and the independent `PolicySnapshot` (ACE engine + revision); acquire
   process admission (`503 gateway.overloaded` when exhausted).
2. Resolve the client IP (trusted proxies only) and charge the mandatory
   `ingress.limit` IP bucket, bounded by `ingress.timeout` (default 250ms):
   `429` on denial, `503` on store failure, before any credential check or route match.
3. Verify API key / JWT / OAuth token and capture the subject.
4. Match compiled v1 routers (descending `priority`, declaration order for ties).
5. Apply route middlewares (path rewrite, preserve host, header transforms).
6. Apply policies in fixed order: auth, access control, rate limit, quota.
7. Build an execution tree that keeps each service's failover rules and mirrors;
   weighted choices and mirror sampling are deterministic per request; upstream
   selection happens when execution enters a leaf.
8. Decide replay eligibility from the method (`GET`, `HEAD`, `OPTIONS`, `TRACE`,
   `PUT`, `DELETE` may fail over; `POST`/`PATCH` dispatch once, regardless of
   `Idempotency-Key`), buffer only when needed, then execute HTTP, WebSocket, or
   direct response and apply response headers.

Modules: `request.rs` (orchestration), `authentication.rs`, `middlewares.rs`,
`routing/`, `policies/` (`ingress.rs`, `limits.rs`), `execution/` (`plan.rs`,
`replay.rs`, `mirrors.rs`, `disposal.rs`), `upstream/` (`attempt.rs` attempt
numbering + health feedback, `http.rs`, `headers.rs`, `internal_context.rs`
per-attempt signing, `websocket/`), `response.rs`, `lifecycle/` (deadlines,
cancellation, guarded bodies), `tests/`, `performance/` (opt-in workloads).

### Gateway Config v1

Minimal `config.yaml`:

```yaml
schema: stargate/v1
ingress:
  limit: ingress
  timeout: 250ms
limits:
  default:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 1s
  ingress:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 100ms
```

Top-level objects: `ingress` (mandatory per-IP admission), `limits` (named
`gcra` / `token_bucket` / `quota_tracker` specs; `default` is required),
`runtime` (budgets/deadlines), optional `mtls` (upstream CA + client identity),
and `http.{upstreams,services,middlewares,policies,routers}`.

- Matchers: method, host, path (`exact`/`prefix`/`template`/`regex`), header,
  query, cookie (`eq`/`prefix`/`suffix`/`contains`/`regex`/`present`/`one_of`),
  source IP/CIDR, and `all`/`any`/`not`.
- Middleware: `strip_prefix`, `add_prefix`, `replace_path_regex`, `preserve_host`,
  `request_headers`, `response_headers`.
- Services: `load_balancer`, `weighted`, `mirror`, `failover` (`on_status`),
  `direct_response`.
- Policies: `auth` (strategies `jwt`, `api_key`, `oauth`; OAuth requires an exact
  `audience` and a live linked session), `access_control`, `rate_limit`, `quota`.
  Org-scoped limits have `on_missing: skip | ip_fallback | deny`.
- Upstreams: `transport.connect_timeout`, `transport.protocols` (`http1`/`http2`;
  WebSocket needs `http1`), `load_balancer.liveness_probe` and `circuit_breaker`
  (`fail_threshold` 3, `cooldown` 30s), optional `internal_context.audience`.
- Routers: `response_mode: stream` for long-lived responses (default `finite`).
- `runtime` defaults: primary 1024 / mirror 64 slots, replay 2 MiB per request /
  64 MiB total, upload idle 15s, connect 5s, response headers 30s, body idle 30s,
  mirror 5s, request 60s. Capacity fields require a restart.
- Subject `attrs.rate_limit` / `attrs.quota` pick named limits; unknown names fail
  with `500 gateway.limit_configuration_invalid` (no silent fallback).

Full reference: `docs/config-v1.md`.

### Traffic Management

Implemented: weighted splits, mirrors (bounded disposal of discarded bodies),
failover on transport error and selected statuses with per-service boundaries,
direct responses, catch-all fallback routes, replay buffering, per-upstream
protocol policy, finite/stream deadlines, WebSocket proxying, upstream mTLS,
signed internal context.

Partial: the circuit breaker (probes + live traffic; transport errors and
502/503/504 trip it, mirrors do not) is per-process, not shared across cluster
nodes, and its trip statuses are not configurable.

Missing: retry with backoff, request hedging, gRPC / gRPC-Web, GraphQL-aware
routing, first-class SSE handling, geo-aware matchers.

### Internal Context

Upstreams with `internal_context.audience` receive a fresh RS256-signed
`stargate-context` header per attempt (`crates/ctx`). The header is reserved:
middleware and direct responses cannot set it. Keys come from
`stargate internal-context generate-key` (never generated at startup); public keys
are at `/.well-known/stargate-context-jwks.json`, separate from the OIDC JWKS.
Docs: `docs/internal-context-contract-v1.md`, `-consumer-guide.md`, `-operations.md`.

### Audit (transactional outbox)

Producers build the versioned raw-event contract and store its immutable JSON
in `outbox_events` in the **same transaction** as the mutation. `crates/db`
mutators take a `TrustedAuditContext` (actor, request facts, scope from
authenticated/persisted state). All `/admin/*` events are `control_plane`.
No in-process buffer. Login/logout events are not produced.

- **Edge**: no relay; rows stay pending (`published_at = NULL`).
- **Cluster**: one relay per process (`src/aud/relay.rs`) claims pending rows in
  `seq` order with `FOR UPDATE SKIP LOCKED`, publishes to `AUDIT_REDIS_STREAM`
  (default `audit.raw`) as a single `payload` field, then marks them published and
  commits. At-least-once: consumers dedupe on `event_id`. Errors roll back and back off.

Super-admins read delivery state via `GET /admin/outbox-events[/{event_id}]` (read-only).

### Sessions

Each login has a stable JWT `sid`, shared by native access/refresh tokens and
linked user OAuth/OIDC tokens. Native refresh rotation stores the current `jti`
under `account:session-refresh:{sid}`. The store keeps a per-user index plus the
`account:sessions:directory` hash (sid, org, times, linked OAuth client ids).
Removing a session invalidates linked OAuth tokens for Stargate's own checks
(admin, userinfo, introspection, OAuth refresh). Admin endpoints list/revoke sessions.

### Multi-Org

Users can belong to several organizations (role convention `owner`/`admin`/`member`);
each session acts in at most one org (`org_id` claim). At login: explicit `orgId`,
then `users.attrs.default_org`, then the oldest membership, else none.
`GET /account/organizations` lists, and switching mints a new session. The org
context feeds ACE and org-level limits. Guide: `docs/multi-org-guide.md`.

### Singletons / Runtime State

- Database pool (`DB`)
- Audit relay (cluster only)
- JWT config
- GeoIP database
- Gateway `RuntimeSnapshot` (`ArcSwap`): graph, balancers, limiter, prepared clients,
  version, generation-owned health probes
- ACE `PolicySnapshot` (engine + `sha256:*` revision)
- Internal-context signer (when configured)

### Config Hot-Reload

`notify` watches `config.yaml` and the ACE policy file; content comparison avoids
false positives.
- A candidate is parsed, compiled, internal-context-preflighted, and fully prepared
  (transports, graph, balancers, limiter, unstarted probes) before publishing.
- On success: next config version, atomic snapshot swap, new probes start, old retire.
  Pinned requests, bodies, mirrors, and WebSockets keep their generation.
- On failure: log, record a bounded reload outcome, keep the previous snapshot.
- Admin config endpoints run the same preparation before saving. Replacing TLS
  files alone does not reload.

ACE rules (`.stargate/policies`) are managed separately via
`/admin/access-control/rules` (GET, PUT with revision, `/validate`, `/evaluate`)
and `/admin/access-control/capabilities/evaluate`; updates do not bump the
gateway config version. `# @id` / `# @description` comments above a rule are metadata.

### Feature Flags

```toml
default = []
edge    = ["sqlite", "memory"]
cluster = ["postgres", "redis"]
```

Compile-time backend selection, mutually exclusive. No runtime backend branching.

## OAuth/OIDC Provider

Admin-managed clients only (no dynamic registration).

Source layout:
- `crates/oidc`: pure provider logic (no Axum or DB driver dependency).
- `src/api/oidc`: Axum bindings for provider endpoints and well-known metadata.
- `src/api/social`: Google/GitHub consumer login callbacks and state endpoint.
- `src/api/admin/oauth_clients.rs`: client CRUD.

Endpoints:
- `/.well-known/oauth-authorization-server`, `/.well-known/openid-configuration`,
  `/.well-known/jwks.json`
- `GET`/`POST /oauth/authorize`
- `POST /oauth/login` (no-JS fallback of the hosted login; see below)
- `GET /oauth/interactions/{id}`, `POST /oauth/interactions/{id}/consent` (consent page API)
- `/oauth/token` (`Cache-Control: no-store`)
- `GET`/`POST /oauth/userinfo` (POST also accepts form `access_token`)
- `/oauth/introspect`, `/oauth/revoke`
- `/admin/oauth/clients/*`

Provider login (one login UI, the hosted auth app):
- `/oauth/authorize` without a usable session parks the request under
  `oauth:authorize-interaction:{id}` (10 min) and redirects to
  `/auth/login?return_to=<issuer>/oauth/authorize?interaction={id}` (+ `login_hint`),
  setting the `oauth_interaction` cookie (`SameSite=Strict`, `Path=/oauth/login`).
- With JS the SPA signs in via `/account/login` and follows `return_to`; a session
  newer than the parked request satisfies `prompt=login` / `max_age`.
- The `apps/auth` build prerenders the login route (`scripts/prerender.mjs`,
  `app/entry-server.tsx`) into `login.html`, served by `src/api/auth_app.rs` for
  `/auth/login`; `app/main.tsx` hydrates it. Keep the first render of hydrated
  routes free of URL/storage reads (`lib/hydration.ts` `useHydrated`).
- Without JS the form posts natively to `POST /oauth/login` (`src/api/oidc/login.rs`):
  password-only, redirects only; MFA / expired password -> `error=javascript_required`.
  The OIDC conformance suite's HtmlUnit browser uses this path.
- SPA `return_to` is restricted to in-app paths and the page/API origins (`safeReturnTo`).

Consent (`src/api/oidc/consent.rs`, SPA `app/routes/consent.tsx`, JS only):
- Non-first-party clients need an active `oauth_consents` row covering the requested
  scopes/audience. Otherwise (or with `prompt=consent`) the request is re-parked with a
  `ConsentStage` bound to the signed-in user and the browser goes to
  `/auth/consent?interaction={id}`; `prompt=none` gets `consent_required`.
- The page reads `GET /oauth/interactions/{id}` and posts `{ approve }` to
  `/oauth/interactions/{id}/consent` (session cookie; other users' ids are `404`).
  Approval upserts the consent (scopes merged), marks the stage approved, and returns
  the resume URL; denial ends the request with `access_denied` to the client.
- A resumed stage skips the login checks for the same user only (they passed before
  parking) and satisfies `prompt=consent` once approved.
- `GET /account/consents` / `DELETE /account/consents/{client_id}` let users list and
  revoke. The refresh grant re-checks consent for non-first-party clients and drops the
  family when it is gone; issued access tokens live until expiry.
- Writes take the user's `TrustedAuditContext` (actor `user`, application scope; the
  service rejects other users) and emit `oauth_consent.granted` / `.updated` / `.revoked`.

Authorization request handling:
- Errors after the client and `redirect_uri` are verified are redirected with
  OAuth codes (`invalid_request`, `unsupported_response_type`, `unauthorized_client`,
  `invalid_scope`, `login_required`, `consent_required`, `interaction_required`,
  `request_not_supported`, `request_uri_not_supported`); before that, `400`.
- Unregistered scopes are dropped; `openid` is required. `max_age=0` = `prompt=login`.
- A replayed authorization code revokes the access token and refresh family it issued.

Signing keys (`src/etc/auth/jwt_keyring.rs`, `crates/jwt/src/keyring.rs`):
- `.stargate/jwks/keys/<kid>/{private.pem,public.pem,alg}`; `JWT_KID` is the active
  key (signs). Every other dir is verification-only: published in the JWKS (active
  first) and accepted for tokens with its `kid`. Verification picks the key by `kid`
  and uses that key's algorithm; unknown `kid` is rejected. Private/public pairs are
  probe-checked at load; inactive keys need only `public.pem`.
- First start generates `keys/<JWT_KID>/` (and `keys/<JWT_ID_TOKEN_KID>/`, RS256) only on
  a fresh keyring; afterwards a missing named key fails startup (no silent new keys).
  `alg` is inferred from the public key when absent. Tokens without a known `kid` are
  rejected. `JWT_ALGORITHM` mismatch with the active key fails startup.
- Rotation: generate next key + restart (published), wait JWKS cache, set `JWT_KID`
  + restart, remove the old dir after `JWT_REFRESH_EXP`. HMAC (`HS*`) is single-key.
- ID tokens: `JWT_ID_TOKEN_KID` names an RS256 key (loaded with its private key,
  `JwtConfig::with_id_token_key`) that signs only ID tokens; discovery advertises
  `id_token_algorithm()`. Works with an EC or HMAC active key. Without it a non-RS256
  active key logs a warning and fails the Config OP discovery check (OIDC requires
  RS256). Rotate it with `jwt generate-key --id-token`, same steps.
- Helm: edge keeps the keyring on the PVC; cluster mounts one Secret per key (`jwt.keys`)
  read-only at `keys/<kid>/`, and `jwt.activeKid` / `jwt.idTokenKid` set `JWT_KID` /
  `JWT_ID_TOKEN_KID` (chart-managed). Cluster needs `jwt.activeKid` unless `JWT_ALGORITHM`
  is HMAC; rotation is one `helm upgrade` per step (`k8s/stargate/README.md`).

Grants: `client_credentials`, Authorization Code + PKCE (`S256`), `refresh_token`
with opaque rotating families.

Scopes/claims:
- `openid`: `sub`
- `email`: `email`, `email_verified` (in userinfo and the ID token)
- `profile`: `name`, `given_name`, `family_name`, `nickname`, `preferred_username`,
  `picture`, `updated_at`
- `offline_access`: refresh tokens when the client allows `refresh_token`
- ID tokens carry no private `typ` / `sub_id` claims.

Provider state:
- SQL: `oauth_clients`, `oauth_consents`.
- Store: `oauth:authorization-code:{code_hash}` (pending/redeemed, TTL),
  `oauth:refresh-family:{family_id}` (CAS rotation), `oauth:authorize-interaction:{id}`,
  and JWT `jti` revocations until expiry.

Client notes:
- Client secrets are returned only on create/rotate and stored as hashes.
- Public clients use `token_endpoint_auth_method = none` and always need PKCE.
- Confidential clients require PKCE unless `attrs.require_pkce = false`; a code
  issued without a challenge rejects a `code_verifier` (downgrade protection).
- First-party clients skip consent with `attrs.first_party` (the only accepted name;
  client attrs use snake_case). The consent page shows `logo_uri` / `client_uri` / `policy_uri` /
  `tos_uri` attrs when they are HTTP(S) URLs.
- Privileged token operations: `oauth:introspect` / `oauth:revoke` scopes or
  `attrs.can_introspect` / `attrs.can_revoke`.
- The session cookie is `SameSite=Lax` so `/oauth/authorize` sees sessions on
  cross-site redirects.
- The Console uses the reserved public client `stargate_console`, created by
  `admin bootstrap`.

Conformance: Config OP and Basic OP plans run in `.github/workflows/oidc-conformance.yml`
(builds `apps/auth` too). Expected warnings/skips are in `.github/oidc-conformance/`.
Docs: `docs/oauth-oidc-provider-guide.md`, `docs/oidc-conformance.md`.

## OTP, Passwordless Login, and MFA

Source layout:
- `crates/otp`: HOTP (RFC 4226), TOTP (RFC 6238), Base32, provisioning URIs,
  short-lived message OTP records for email/SMS.
- `src/act/otp`: policy evaluation, challenge persistence, verification, delivery,
  throttling, service flows. Workflow changes go here.
- `src/api/account/otp`: thin Axum/OpenAPI bindings.

Account endpoints:
- `POST /account/login/otp/email` - start passwordless email OTP (generic response for unknown users).
- `PUT /account/login/otp/email` - verify and issue a session when MFA policy allows passwordless.
- `PUT /account/login/mfa/challenges/{challenge_id}` - complete login MFA.
- `GET /account/mfa/methods` - effective MFA policy/methods.
- `POST /account/mfa/challenges` - authenticated challenge (email only).
- `PUT /account/mfa/challenges/{challenge_id}` - verify and store a step-up marker.

Login behavior:
- `POST /account/login` verifies the password first; if MFA is required it returns
  `202` with `mfaRequired`, `challengeId`, method, expiry, TTL, max attempts.
- Passwordless email OTP is disabled when MFA is required (no single-factor downgrade).

MFA policy:
- `MFA_MODE`: `off`, `optional`, `required`. In `optional`, users opt in with
  `users.attrs.mfa.enabled = true`; super-admins need MFA when
  `MFA_REQUIRED_FOR_SUPER_ADMIN=true`.
- User attrs: `{ "mfa": { "enabled": true, "methods": ["email"], "preferredMethod": "email" } }`.
- `MFA_ADMIN_STEP_UP_REQUIRED=true`: super-admin user sessions need an `admin`
  step-up marker before admin grants (admin API keys unaffected). TTL `MFA_STEP_UP_TTL_SECS`.

OTP state keys: `account:login:otp:email:{challenge_id}`,
`account:mfa:pending-login:{challenge_id}`, `account:mfa:challenge:{user_id}:{challenge_id}`,
`account:mfa:verified:{sid}:{purpose}`, throttling under `account:otp:request:*`.

Delivery: `smtp::Smtp` on `spawn_blocking`; passwordless delivery is backgrounded
against enumeration timing. Only email OTP is wired; TOTP/HOTP and SMS exist as
primitives only.

Per-recipient email throttle (`src/act/email_throttle.rs`): unauthenticated flows
that email an address from the request body (`POST /account/credentials`,
`POST /signup`, `POST /account/login/otp/email`) charge
`account:email:recipient:{flow}:{sha256(lowercased, trimmed address)}` before any
account lookup, so denials look the same for unknown addresses. Limits per hour:
5 for password reset and signup, 10 for passwordless OTP; flows have separate
buckets. Over-limit requests are not counted (the window does not slide under a
flood) and get `429 email.recipient_rate_limited` with `Retry-After: 3600`.
Admin invitations reuse `send_signup_request` without this throttle.

## Passwords

Argon2id with tunable params and optional pepper (hash upgrades on login), global
password policy from `PASSWORD_POLICY_*` with per-org override in
`organizations.attrs.password_policy`, history in `credential_history`.
Guide: `docs/password-policies-guide.md`.

Forgot password (`POST /account/credentials`): always `200 password.reset_requested`,
whether or not an account uses the address (only exact email matches get a link,
never nickname/phone). The JWT and SMTP delivery run on `spawn_blocking`, so
response time does not reveal the account either; delivery errors are only logged.
A pending request (`change_password_requests:{email}`, 1h) is reused and its link
resent, bounded by the per-recipient throttle, so a failed delivery can be retried.

Signup (`POST /signup`): always `200 signup.requested` for a valid address
(`400 request.invalid` otherwise). In the background a new address gets the
verification link, a pending signup has its link resent (same `sid`), and an
existing account gets a `SignupAccountExists` sign-in reminder, so neither the
response nor its timing reveals the account. Admin invitations
(`send_invitation`) keep `409` conflicts and wait for delivery.

Emails: templates come from `apps/mail/emails/*.tsx` (`npm run export` writes
`.stargate/transactional/*.html`). Action URLs are built by the server
(`src/fun/auth_links.rs`: `signup_url`, `change_password_url`, `login_url`, based on
`AUTH_PUBLIC_BASE_URL` or the local auth app) and rendered as `{{link}}`; `{{name}}`
is the only other placeholder.

## Database Schema (migrations/)

Tables: `organizations`, `users`, `super_admins`, `credentials`, `credential_history`,
`api_keys`, `admin_keys`, `service_accounts`, `oauth_clients`, `oauth_consents`,
`user_organizations`, `user_api_keys`, `service_account_api_keys`, `outbox_events`.
One migration per backend (`migrations/{postgres,sqlite}/..._init_schema.sql`)
creates the full schema; add new forward migrations from there.

User identity (`crates/db/src/ent/user.rs`, enforced by the users repository on
every insert/update):
- `email` and `nickname` are stored canonical (trimmed, lowercased:
  `canonical_email` / `canonical_nickname`) and looked up the same way, so one
  mailbox or handle is one account whatever the casing.
- `nickname_is_valid`: a nickname is non-empty and may contain `@` only when it
  equals the account's own email (the default nickname), so no account can take
  another person's address as its handle. A nickname that mirrored the email
  follows it when the email changes (`update_user`).
- Login (`get_user_by_login`) dispatches by shape: an identifier with `@` matches
  emails only, anything else nicknames only. Phone numbers are unverified profile
  data and never log in.
- Flows keyed by an address (signup, invitations, forgot/reset, passwordless,
  social login) use `get_user_by_email`; session flows (profile, refresh) use
  `sub_id`. API writes pre-check with `act::nicknames::ensure_nickname_available`
  (`400 user.nickname_invalid`, `409 user.nickname_already_exists`).
Store keys derived from emails use the canonical form too.

IDs are ULIDs stored as text. `outbox_events` holds the immutable JSON payload,
`seq`, optional `operation_id`, `created_at`, and nullable `published_at`, with
the same logical columns on PostgreSQL and SQLite.

## Key Env Vars

| Var | Default | Purpose |
|-----|---------|---------|
| `PORT` | 5050 | HTTP listen port |
| `STARGATE_RUNTIME_PROFILE` | - | `edge` or `cluster` (validated against the build) |
| `JWT_ALGORITHM` | active key / `RS256` | Must match the active key when set; picks the first generated key |
| `JWT_SECRET` | - | HMAC key for HS* algorithms |
| `JWT_ISSUER` | `http://localhost:$PORT` | JWT and OAuth/OIDC issuer |
| `JWT_KID` | `stargate-current` | Active signing key (`.stargate/jwks/keys/<kid>/`) |
| `JWT_ID_TOKEN_KID` | - | Optional RS256 key that signs ID tokens (EC or HMAC active key) |
| `JWT_ACCESS_EXP` / `JWT_REFRESH_EXP` | 1h / 1d | Token TTLs |
| `OAUTH_BASE_URL` | issuer | Public base URL for provider endpoint links |
| `JWKS_CACHE_MAX_AGE_SECS` | 300 | JWKS cache max-age |
| `INTERNAL_CONTEXT_*` | see `.env.example` | Internal-context issuer, kid, key/JWKS paths, TTL, skew, signing workers/queue |
| `POSTGRES_ENDPOINT` / `_USERNAME` / `_PASSWORD` / `_DATABASE` | - | Postgres (cluster) |
| `REDIS_URL` | - | Redis (cluster) |
| `RUST_LOG`, `LOG_DIR`, `LOG_FILE_ENABLED`, `LOG_CONSOLE_ENABLED` | - | Logging |
| `OTEL_ENABLED` | auto | OTLP export; on when an endpoint is set |
| `OTEL_EXPORTER_OTLP_ENDPOINT` / `_PROTOCOL` | - / grpc | OTLP target (per-signal overrides exist) |
| `AUDIT_RELAY_ENABLED` | true | Run the outbox relay (cluster) |
| `AUDIT_RELAY_BATCH` / `AUDIT_RELAY_INTERVAL_MS` | 256 / 1000 | Relay batch size / idle poll |
| `AUDIT_REDIS_STREAM` | audit.raw | Redis stream for raw audit events |
| `AUDIT_STREAM_MAXLEN` | 100000 | Approx stream cap (`MAXLEN ~`; 0 = unbounded) |
| `TLS_ENABLED`, `TLS_CERT`, `TLS_KEY` | false | Listener TLS |
| `TRUSTED_PROXIES` | - | Proxy IP/CIDRs allowed to set client IP headers |
| `CORS_ORIGINS` | - | Allowed CORS origins |
| `TRUSTED_ORIGINS` | - | Trusted browser origins for sensitive IAM endpoints |
| `GEOIP_DB_PATH` | - | MaxMind GeoLite2 |
| `SERVER_DRAIN_DELAY_SECS` | 5 | Readiness-off delay before listeners close |
| `SERVER_SHUTDOWN_TIMEOUT_SECS` | 25 | Connection drain deadline |
| `AUTH_APP_DIR` / `AUTH_APP_BASE_PATH` | `.stargate/apps/auth` / `/auth` | Hosted auth app files and mount |
| `AUTH_PUBLIC_BASE_URL` | - | Public auth app URL when hosted elsewhere |
| `CONSOLE_APP_DIR` / `CONSOLE_APP_BASE_PATH` | `.stargate/apps/console` / `/stargate` | Console files and mount |
| `CONSOLE_OAUTH_CLIENT_ID` | stargate_console | Reserved Console OAuth client |
| `CONSOLE_OAUTH_REDIRECT_URI`, `CONSOLE_PUBLIC_URL` | derived | Console callback / origin |
| `EMAIL_OTP_PEPPER`, `EMAIL_OTP_LENGTH`, `EMAIL_OTP_TTL_SECS`, `EMAIL_OTP_MAX_ATTEMPTS` | -, 6, 300, 5 | Email OTP |
| `MFA_MODE` | optional | `off`, `optional`, `required` |
| `MFA_METHODS` / `MFA_DEFAULT_METHOD` / `MFA_EMAIL_ENABLED` | email / email / true | MFA methods |
| `MFA_REQUIRED_FOR_SUPER_ADMIN` | true | MFA for super-admins in `optional` mode |
| `MFA_ADMIN_STEP_UP_REQUIRED` / `MFA_STEP_UP_TTL_SECS` | false / 300 | Admin step-up |
| `PASSWORD_POLICY_*` | see `.env.example` | Global password policy |
| `ARGON2_MEMORY_KIB` / `_ITERATIONS` / `_PARALLELISM` | 19456 / 2 / 1 | Argon2id cost |
| `PASSWORD_PEPPER` | - | Optional Argon2 secret |
| `SMTP_*`, `GOOGLE_OAUTH_*`, `GITHUB_OAUTH_*` | - | Email delivery, social login |

Full list: `.env.example`.

## Build & Run

There is no default profile: `edge` and `cluster` are mutually exclusive, so
exactly one must be selected on every build/run. Bare `cargo run` fails with a
"must be built with one backend" error by design.

```bash
# Dev (edge/sqlite)
cargo run --features edge

# Release
cargo build --release --features edge
cargo build --release --features cluster

# Docker (also builds console, auth, mail apps)
docker build --build-arg STARGATE_PROFILE=edge -t stargate:edge .
docker build --build-arg STARGATE_PROFILE=cluster -t stargate:cluster .

# Hosted auth app (writes .stargate/apps/auth, including the prerendered login.html)
cd apps/auth && npm ci && npm run build
```

## CLI

```bash
# First super-admin + reserved Console OAuth client (idempotent)
cargo run --features edge -- admin bootstrap \
  --email admin@example.com --generate-password \
  --name "Admin User" --nickname admin \
  --oauth-redirect-uri https://identity.example.com/stargate/auth/callback

# Next JWT signing key in .stargate/jwks/keys/<kid>/;
# --id-token creates the RS256 key for JWT_ID_TOKEN_KID
cargo run --features edge -- jwt generate-key --kid stargate-2026-11

# Internal-context signing key + public JWKS (refuses to overwrite)
cargo run --features edge -- internal-context generate-key \
  --output-dir .stargate/internal-context --kid stargate-internal-current
```

Bootstrap flags: `--password`, `--password-stdin`, `--generate-password`,
`--oauth-client-id`, `--oauth-redirect-uri`. `jwt` and `internal-context` commands
need no database. Docs: `docs/cli.md`.

## API Overview

- `GET /livez` (process liveness), `GET /readyz` (initialized, not draining, SQL and
  cluster Redis up). See `docs/health-probes.md`.
- `GET /docs` (Swagger UI), `GET /docs/errors` (error catalog), `GET /i18n/{locale}` (`en`, `it`)
- `/.well-known/*`: OAuth/OIDC metadata, JWKS, `stargate-context-jwks.json`
- `/account/*`: login, logout, profile, refresh, credentials, organizations, consents, OTP/MFA
- `/oauth/*`: provider endpoints; `/oauth/state`, `/oauth/github`, `/oauth/google`: social login
- `/signup/*`: registration + email verification
- `/admin/*`: users (+ invitations), orgs, API/admin keys, OAuth clients, service accounts,
  config, access-control rules, sessions, outbox events, `me`, `overview`, `health`.
  Super-admin users bypass capability checks; admin keys need exact `<resource>:<action>`
  permissions (`src/api/admin/authorization.rs`).
- `/auth/*` (hosted auth app), `/stargate/*` (console), everything else: gateway fallback.

`api::server_router` applies the IAM ingress limit and merges probe routes outside
it; `main.rs` adds security headers, tracing, CORS, compression, and normalize-path.

IAM limits (`src/etc/http/rate_limit.rs`): every IAM route charges the client IP's
ingress allowance, the same `ingress:ip:{ip}` bucket as the gateway, before
credentials are read (`429 iam.ingress_rate_limit_exceeded`). Authenticated
`/admin/*` principals then charge their own `default` bucket, keyed
`lim:admin:user:{id}` / `lim:admin:key:{id}` in `extract_admin_authorization`
(`429 iam.rate_limit_exceeded`, scope `subject`), so one admin key cannot throttle
the console or other keys behind the same IP. Limiter failures: `503 iam.limiter_unavailable`.
Login brute force is handled separately by `act/login_guard.rs` and OTP throttles.

## Error Response Format

JSON body plus `x-request-id` header:

```json
{
  "message": "User not found",
  "code": "user.not_found",
  "type": "not_found",
  "link": "https://docs.stargate.dev/errors/user.not_found",
  "params": { "id": "01J..." }
}
```

`code` is the stable contract and i18n key (definitions in `src/err/types.rs`);
`message` is the English fallback; `params` are interpolation values; `type` is a
broad category, not for application logic; `link` base is `DOCS_URL`. OAuth
endpoints use the RFC 6749 `{ "error", "error_description" }` envelope
(`OAuthErrorResponse`).

Statuses in use: 400, 401, 403, 404, 405, 408, 409, 413, 415, 429, 500, 502, 503, 504.

## Rate Limit Response Headers

`x-ratelimit-limit`, `x-ratelimit-remaining`, `x-ratelimit-scope`, `x-quota-limit`,
`x-quota-remaining`, `x-quota-scope`, `retry-after`. Scope is `subject`, `org`, `ip`,
or `ingress`. `429` `retry-after` is whole seconds rounded up (missing duration
-> 60); `503` uses 10. Shared helpers (`ingress_key`, `limited_response`,
`insert_limit_headers`) live in `src/etc/limits.rs`.

## Graceful Shutdown

SIGTERM/SIGINT -> readiness off -> wait `SERVER_DRAIN_DELAY_SECS` -> gateway admission
closes and active work is cancelled -> listeners close and connections drain
(`SERVER_SHUTDOWN_TIMEOUT_SECS`) -> tracked mirror/WebSocket tasks drain -> stop
audit relay -> save in-memory store -> flush telemetry.

## Frontend (apps/)

`apps/` holds the TypeScript frontends, separate from the Rust binary:

- `console`: super-admin console (users, orgs, API/admin keys, OAuth clients,
  service accounts, sessions, audit outbox, gateway config, access-control
  policies). Reference implementation of the layering below. Served at `/stargate`.
- `auth`: hosted login / auth-flow UI (password login, passwordless email OTP,
  login MFA, reset/change password, organization choice). Same stack and
  layering. Served at `/auth`; its login route is prerendered and hydrated (see
  "Provider login"), other routes are client-rendered from `index.html`.
- `mail`: transactional email templates (`react-email`). A build-time template
  project, **not** a runtime SPA; it does not follow the layering below.

Stack (console/auth): React 19, Vite, TypeScript, Mantine v9
(`@mantine/core|form|hooks|notifications`), Zustand v5, react-router v7, axios,
react-icons (console also `@tanstack/react-table`). Path alias `@/*` maps to the
app root (`@/services`, `@/store`, `@/components`, `@/i18n`, `@/lib`). Build-time
API base URL comes from `VITE_API_URL` (empty = same origin).

Commands (inside `apps/console` or `apps/auth`): `npm run dev`, `npm run build`,
`npm run lint` (oxlint), `npm run format` (prettier). The auth build also runs
`scripts/prerender.mjs`.

The console home uses `/admin/me` as its authentication probe so a dependency
outage does not block the shell. It polls `/admin/health` separately and loads
`/admin/overview` plus recent outbox events for the dashboard.

### Layered Architecture

Strict, **one-way** data flow; each layer depends only on the layer to its right:

```
components/          app/routes/ (pages)      store/                services/
(agnostic UI)   <-   (render + wiring)   <-   (state + actions)  <-  (raw API)
```

- Only `store/` imports `services/`.
- Only `app/routes/` (pages) import `store/`.
- `components/` import **neither** the store nor services; they are driven by props.
- DTOs in `services/types.ts` are the shared contract imported by every layer.

Keep each concern in exactly one layer: data shaping, the loading/error
lifecycle, notifications, and "what happens next" live in the store; pages only
wire; components only render.

#### 1. `services/`: raw API (no React, no state)

- `http.ts`: the `Http` class over axios. `authRequest<T>()` attaches bearer auth
  with proactive/reactive refresh via `TokenManager`; `request<T>()` is for
  unauthenticated calls. Every call returns `Response<T> = { data, error?, message?,
  status?, headers? }`; HTTP/error outcomes are captured, never thrown.
- `api.ts`: `AdminApiService`, thin endpoint methods (`getOrganizations`,
  `createOrganization`, ...) returning `Response<T>`.
- `index.ts`: lazy `services.admin` getter wiring `Http` + `TokenManager` +
  `AdminApiService` from runtime config; tokens persist through `lib/oauth.ts`.
- `types.ts`: request/response DTOs. `token-manager.ts`, `jwt.ts`: token helpers.

#### 2. `store/`: Zustand state + actions (the only caller of services)

- `index.ts`: one store, `create()` + `devtools`; `initialState` holds each entity
  as a `StoreItem`.
- `types.ts`: `State` and `Actions` interfaces (the store contract).
- `item.ts`: `StoreItem<Data>` wraps async slices as `{ data, meta, status, message }`
  with `setLoading()` / `setError(msg)` / `setSuccess(data)` and
  `isLoading()` / `isError()` / `isSuccess()` / `isIdle()`.
- `settings.ts`: localStorage prefs (theme, language). `accessor.ts`: dot-path helpers.

Actions follow one shape: flip the item to loading, call the service, branch on
the envelope; mutations also notify and re-fetch:

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
  get().getOrganizations();
},
```

#### 3. `app/routes/`: pages (render + action wiring)

- `app.tsx`: the `createBrowserRouter` table (auth: `route-objects.tsx`, shared
  with the prerender).
- A page pulls state + actions from `useStore()`, loads on mount, keeps only
  local UI state, and composes components, passing actions as callbacks. Labels
  come from `useTranslations()` (`@/i18n`).
- Page-specific files stay co-located and unshared: `columns.tsx`,
  `create-*.tsx` / `edit-*.tsx` (`@mantine/form` drawers that build the DTO),
  `options.ts` / `*-utils.ts`.

#### 4. `components/`: agnostic, reusable UI

- Barrel `components/index.ts` (`Table`, `EntityDrawer`, `Shell`, `CodeBox`, `Icon`,
  `JsonAttributesForm`, `showNotification`, `useConfirmModal`, ...).
- Props/children only; no store, services, or domain-type imports. A component
  that needs `@/store` or `@/services` belongs in `app/routes/`.

Example (add a field to Organizations): extend the DTO in `services/types.ts`;
`services/api.ts` only changes if the endpoint does; adjust store types/actions
only if the signature is new; add the form field in
`app/routes/organizations/create-organization.tsx`; touch `components/` only for
a new reusable primitive.

## Current Notes

- Request ID: ULID per request, `x-request-id`, also in audit events.
- ACE decision caches are version-aware and invalidated on policy change.
- GeoIP (MaxMind GeoLite2-City) is available but not a v1 route matcher.
- Metrics use bounded labels only (no URLs, credentials, request IDs); see
  `docs/config-v1.md` for gateway instruments.
- Gateway performance workloads are opt-in (`src/api/gateway/performance/`,
  `benches/scripts/`).
