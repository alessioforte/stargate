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
│   ├── api/           # Axum routers + handlers; IAM middleware; apps/ auth + console hosting
│   ├── aud/           # PostgreSQL -> Redis audit outbox relay (cluster only)
│   ├── cli/           # CLI: admin bootstrap, config export/import, internal-context / jwt generate-key
│   ├── conf/          # Process-wide configuration: env readers, feature toggles, config dir
│   ├── ctx/           # Request context, propagation drafts, audit/authz projections; sign/ signer
│   ├── data/          # Persistence backends: db/ (SQL pool + repository service), store/
│   ├── err/           # Error catalog (ErrorCode) -> JSON responses; OAuth error envelope
│   ├── gate/          # Gateway runtime: snapshot lifecycle (snap/), request pipeline, transports
│   ├── iam/           # Identity and access services: auth, authz (ACE), sessions, users,
│   │                  #   passwords, OTP/MFA, federation state, OIDC client calls, bootstrap
│   ├── obs/           # Logging and telemetry (traces, metrics, logs)
│   ├── srv/           # Listeners, listener TLS, health, profile, banner, shutdown
│   └── web/           # Shared HTTP support: request, proxies, CORS/origins, headers, cookies, links
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
├── docs/              # Guides: config-v1, features, oauth-oidc-provider, oidc-conformance, multi-org,
│                      #   password-policies, health-probes, internal-context-*, cli, docker, api errors,
│                      #   gateway-performance (opt-in benchmarks and recorded results)
├── k8s/               # Kubernetes manifests
└── benches/           # Criterion benchmarks; scripts/ has gateway/backend benchmark runners
```

Module boundaries in `src/` (each top-level name is 3-4 characters):
- `main.rs` composes the subsystems; `api` (HTTP) and `cli` invoke them.
- A function lives with the subsystem whose rules it implements; there are no
  catch-all modules (`utils`, `helpers`, `common`). Prefer private modules and a few
  explicit `pub(crate) use` re-exports over glob re-exports.
- `iam` imports neither `api` nor `gate`; the gateway uses `iam`. `web` is shared HTTP
  plumbing and orchestrates no login, signup, or upstream request. `ctx` describes,
  projects, and signs request facts; it decides no authorization. `data` depends on no
  handler; audit events are written in the repository transaction and `aud` only
  delivers them.
- `conf` holds process-wide settings only; a subsystem's settings stay with it
  (`gate/snap/config.rs`, `iam/otp/settings.rs`, `ctx/sign/settings.rs`, `iam/auth/jwt.rs`).
- The local `gate` and `ctx` modules share their names with the `gate` and `ctx` crates
  (as `data/db`, `data/store`, `iam/oidc`, `iam/otp` do one level down). Only the file
  that declares the module sees the local one under the bare name (`main.rs`,
  `data/mod.rs`, `iam/mod.rs`); elsewhere `gate::cfg` is the crate and the local module
  is `crate::gate`.

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
- **OpenAPI**: utoipa
- **Observability**: tracing + OpenTelemetry (OTLP traces, metrics, logs)

### Identity and Access (`src/iam/`)

Application services for identity and access. `api` handlers and the gateway call
them; `iam` imports neither `api` nor the local gateway runtime.

- `auth/`: subjects, verified identity facts (`identity.rs`), credential/backing-state
  checks (`verification.rs`), session JWT config and keyring (`jwt.rs`,
  `jwt_keyring.rs`), login guard, JWT revocation (`revocation.rs`).
- `authz/`: ACE evaluation (`evaluation.rs`), rule management (`rules.rs`), the
  database-backed policy documents and their seed (`storage.rs`), super-admin checks (`admin.rs`).
- `sess/`: sign-in (`issuance.rs`: `start_user_session` returns the tokens; `api`
  adds the cookie and JSON), session registry (`registry.rs`), token minting (`tokens.rs`).
- `user/`: signup requests, email verification, nicknames, display names (`profile.rs`).
- `pwd/`: Argon2 wrappers (`hash.rs`), effective policy, expiry and history
  (`policy.rs`), reset requests (`reset.rs`).
- `otp/`: OTP/MFA flows (see "OTP, Passwordless Login, and MFA").
- `fed/`: social sign-in state (`state.rs`).
- `oidc/`: provider state (codes, refresh families, parked interactions), the
  authorization request, back-channel logout, client JWKS, the outgoing client for
  OAuth client endpoints (`http_client.rs`).
- `boot.rs`: first super-admin and Console client bootstrap. `email_throttle.rs`: see
  "OTP, Passwordless Login, and MFA".

### Shared Support (paths under `src/`)

- `conf/`: strict env readers (`env.rs`), the config directory (`config_dir`,
  `CONFIG_PATH`), the public base URL (`public_base_url`: `OAUTH_BASE_URL`, else the
  issuer; never the listening host), the hosted apps' base paths (`apps.rs`, read by the app routers,
  links, and the Console's default callback), IAM host selection (`hosts.rs`,
  `IAM_HOSTS`, validated once at startup), and feature toggles (`features.rs`, below).
- `ctx/`: request metadata and ingress headers (`request.rs`), lazy GeoIP; signed-context
  drafts (`propagation.rs`) and audit/authorization projections (`adapters.rs`).
  `ctx/sign/`: internal-context signer runtime, `settings.rs`, `material.rs` key/JWKS
  validation, `signing.rs` bounded CPU workers.
- `data/db/`: SQL pool init, PostgreSQL endpoint/TLS/creation settings (`settings.rs`),
  and the repository service wrappers (`svc.rs`). PostgreSQL fields go directly into
  `PgConnectOptions`, including unencoded credentials. Optional database creation
  uses the same credentials/TLS through the `postgres` maintenance database;
  concurrent-create errors are benign only for `42P04`, or `23505` on
  `pg_database_datname_index`, after a fresh existence query confirms the target.
  `data/store/`: selected profile interface; `memory.rs` persistence, `redis.rs`
  init/readiness.
- `obs/`: logging (`logging/settings.rs` parses the format and sinks), and the
  telemetry provider (`telemetry/metrics.rs`, `telemetry/propagation.rs`). Logs go
  to stdout by default; `LOG_FORMAT` defaults to pretty in debug and JSON in
  release. Optional files remain JSON. ANSI requires a stdout terminal and a text
  format. The server validates settings before its banner and omits it in JSON
  mode; CLI output keeps its existing behavior. JSON includes the current span
  and span list: request logs retain `request_id`, including in nested spans, and
  record the valid SDK `trace_id` when OpenTelemetry is active. With it disabled,
  `trace_id` stays absent; no synthetic trace id is generated.
- `srv/`: listeners, listener TLS, health lifecycle/probes (`health.rs`), profile
  validation, startup banner, shutdown signal and drain (`shutdown.rs`).
  `tls::served_over_https()`: listener TLS or an `https://` public base URL (a proxy
  terminates TLS); it decides the `Secure` cookie attribute, HSTS, and the request's own
  origin scheme.
- `web/`: request extraction (`request.rs`), trusted proxies, origins/CORS (`origin.rs`
  trusted-origin middleware), security and retry headers, the session cookie
  (`cookies.rs`), base and auth-app URLs (`links.rs`), limit headers and `429` answers
  (`limits.rs`), message envelopes.
- `api/middleware/`: IAM limits (`rate_limit.rs`, see "API Overview") and the feature
  gate (`features.rs`).
- `conf/features.rs`: capabilities an operator can switch off, read and validated once at
  startup (invalid values stop it). A disabled capability keeps its routes mounted and
  answers `404 feature.disabled` (`require_feature` middleware and `disabled` answer in
  `api/middleware/features.rs`): unmatched paths fall through to the gateway. Today:
  `SIGNUP_MODE` / `SIGNUP_ALLOWED_DOMAINS` (see "Passwords" > Signup),
  `PASSWORDLESS_LOGIN_ENABLED` (see "OTP, Passwordless Login, and MFA"),
  `CONSOLE_ENABLED`, and each social provider (on when its client id and secret are both
  set; one alone stops startup). `GET /account/features` (public) lists the sign-in
  options for the hosted apps; `/admin/health` reports `features`.
  Guide: `docs/features.md`; plan: `.reports/feature-toggles-plan.md`.

### Gateway Runtime (`src/gate/`)

`gate::init` builds the runtime; `gate::service` is mounted as the Axum
`fallback_service`. Request flow:
1. Pin the active `RuntimeSnapshot` (graph, balancers, limiter, prepared clients,
   version) and the independent `PolicySnapshot` (ACE engine + revision); acquire
   process admission (`503 gateway.overloaded` when exhausted).
2. Resolve the client IP (trusted proxies only) and charge the mandatory
   `ingress.limit` IP bucket, bounded by `ingress.timeout` (default 250ms):
   `429` on denial, `503` on store failure, before any credential check or route match.
   Capture the original peer and host and strip `Forwarded`, `X-Real-IP`, and
   every `X-Forwarded-*` field from untrusted or missing peers before routing
   and middleware. Operator middleware may then supply forwarding metadata.
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
   direct response and apply response headers. Before each upstream dispatch,
   complete forwarded headers using the original connection peer and host:
   append the peer IP once, preserve middleware output, and fill missing
   proto/host values with the actual listener TLS scheme and original host.
   These facts survive replay, failover, and mirrors; normalization precedes
   internal-context sanitization and signing.

Modules: `snap/` (snapshot lifecycle: `config.rs` database documents, seed, preparation and saves,
`runtime.rs` preparation/publication, `reload.rs` text activation, `body.rs` pinning during streaming),
`resources.rs` (process admission, memory permits, tracked tasks), `transport/`
(prepared outgoing HTTP/TLS/mTLS clients), `limits.rs` (ingress bucket key, limit
selection/validation), `request.rs` (orchestration), `authentication.rs`,
`middlewares.rs`, `routing/`, `policies/` (`ingress.rs`, `limits.rs`, `org_limits.rs`
cached org overrides, invalidated through `gate::invalidate_org_limits`),
`execution/` (`plan.rs`, `replay.rs`, `mirrors.rs`, `disposal.rs`), `upstream/`
(`attempt.rs` attempt numbering + health feedback, `http.rs`, `headers.rs`,
`internal_context.rs` per-attempt signing, `websocket/`), `response.rs`,
`lifecycle/` (deadlines, cancellation, guarded bodies), `tests/`, `performance/`
(opt-in workloads).

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
  `forward_credentials` defaults to false, removing Authorization, API keys,
  the `jwt` cookie and token query parameters before dispatch. Other cookies
  use tolerant byte-based removal unless internal context requires strict parsing.
  Opt in for a backend that validates the credentials;
  this cannot be combined with internal context. Proxy-Authorization and
  Connection-nominated fields are always removed as hop-by-hop headers.
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
Every function in `iam/sess/registry.rs` that ends sessions (`end_session` for a user's
own logout, `revoke_session`, `revoke_all_sessions`, `revoke_org_sessions`) also
notifies the linked clients through OIDC Back-Channel Logout (`iam/oidc/backchannel_logout.rs`).
Each record also keeps how the user signed in (`amr`, RFC 8176), set by the sign-in
path through `start_user_session` (`iam/sess/issuance.rs`) / `NewSession`: `pwd`,
`pwd otp mfa` (login MFA), `otp` (passwordless), `fed` (Google/GitHub). An email code verified in the session
(`PUT /account/mfa/challenges/{id}`) calls `add_otp_factor` (`oidc::authentication::with_otp`);
an org switch keeps the methods.

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
  version, exact document revision, generation-owned health probes
- ACE `PolicySnapshot` (engine + `sha256:*` revision)
- Internal-context signer (when configured)

### Configuration Storage and Activation

Gateway configuration and ACE rules come from the active `config_documents` rows in SQL.
On an empty database, startup seeds each kind once: the gateway from
`CONFIG_PATH/CONFIG_FILENAME`, and policies from `CONFIG_PATH/policies` (empty when absent).
A missing gateway seed fails startup. Existing documents take precedence over files.
Seeds are parsed and validated before persistence; an invalid seed creates neither
a document nor an audit event, so correcting the seed allows startup to retry.
Server startup and activation never create or write configuration or policy seed files
and use no file watchers. Explicit CLI exports can write the requested backup files.
Invalid active documents fail startup.

Admin configuration reads return the document revision, version, update time/actor, and
parsed config (or the stored YAML with `?format=yaml`), with an ETag. Writes require the
current revision, fully prepare the candidate before saving it, and append the version
with its safe audit event in one transaction. Conflicts return
`409 configuration.revision_conflict`. Successful saves publish immediately on the receiving
replica. Capacity changes are saved with `activation: restart_required` and retain the
current runtime until restart. Pinned requests, bodies, mirrors, and WebSockets keep
their generation. Replacing TLS files alone does not reload prepared transports.
An unchanged save keeps the existing generation when its document revision is already
active on this replica; it still activates the document when the replica is behind.
Cluster replicas subscribe to PostgreSQL `stargate_config` notifications emitted in the
same transaction as each new document version. A notification or listener reconnection
checks the active document revisions; `CONFIG_SYNC_INTERVAL_SECS` (default 30, integer >= 1)
provides periodic recovery from missed notifications. Failed activations retain the prior
snapshot and retry on the next tick; duplicate notifications do not retry the same failed
revision. Activation failures log once per revision, with a warning when a restart is
required. Database/listener errors are retried without stopping synchronization. Edge runs
the same periodic checks without a listener, so imports from another process sharing
its SQLite file apply without restarting. Synchronization stops
before the audit relay during shutdown. `/admin/health` reports this replica's active
`config.gatewayRevision` and `config.policiesRevision`.

ACE rules are managed separately via `/admin/access-control/rules` (GET, PUT with revision,
`/validate`, `/evaluate`) and `/admin/access-control/capabilities/evaluate`. Their database
updates publish the policy snapshot locally and propagate to cluster peers without bumping
the gateway config version. `# @id` / `# @description` comments above a rule are metadata.

### Kubernetes Deployment

The Helm chart (`k8s/stargate`) optionally deploys pattern A routing through
Traefik v3 `IngressRoute` resources (`traefik.enabled`, mutually exclusive with
the standard Ingress). `values-traefik.yaml` is a three-replica cluster example
with ingress restricted to Traefik and explicit egress rules. The chart manages
`IAM_HOSTS` from the identity host, `TRUSTED_PROXIES` from a nonempty peer list,
and the four signer path/issuer/kid variables when `internalContext.enabled`,
mounting the existing signing Secret read-only. Active managed settings reject
duplicates in runtimeConfig data and extraEnv. Optional gRPC routes use h2c with
higher priority; ordinary HTTP and WebSocket routes keep HTTP transport. Guide:
`k8s/stargate/README.md`, "Pattern A: Traefik in front".

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
- `src/iam/oidc`: provider state on the selected store (authorization codes + PKCE check
  in `codes.rs`, refresh families in `refresh.rs`, parked requests and their consent/
  step-up stages in `interactions.rs`), the authorization request (`request.rs`).
- `src/api/oidc`: Axum bindings for provider endpoints and well-known metadata;
  `interactions.rs` there is the browser side (interaction cookie, hosted page URLs).
- `src/api/social`: Google/GitHub consumer login. Browsers start at
  `/oauth/{provider}/start?return_to=` (state bound to an HttpOnly `social_state` cookie,
  `Path=/oauth`), the callback `/oauth/{provider}` signs in (`start_user_session`, amr
  `fed`) and redirects to the auth app's org picker then `return_to`, or to its login
  page with `error=social_*`; `return_to` must be a path or a URL on the issuer /
  `OAUTH_BASE_URL` / `AUTH_PUBLIC_BASE_URL` / `TRUSTED_ORIGINS` / `CORS_ORIGINS` origins or
  the one the request reached (else `400`, logged). `POST /oauth/state` states (API
  clients) get JSON tokens.
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
- `GET`/`POST /oauth/logout` (`end_session_endpoint`), `POST /oauth/logout/confirm`
- `/admin/oauth/clients/*`

Provider login (one login UI, the hosted auth app):
- `/oauth/authorize` without a usable session parks the request under
  `oauth:authorize-interaction:{id}` (10 min) and redirects to
  `/auth/login?return_to=<issuer>/oauth/authorize?interaction={id}` (+ `login_hint`),
  setting the `oauth_interaction` cookie (`SameSite=Strict`, `Path=/oauth/login`).
- With JS the SPA signs in via `/account/login` and follows `return_to`; a session
  newer than the parked request satisfies `prompt=login` / `max_age`.
- The `apps/auth` build prerenders the login, logout, and signed-out routes
  (`scripts/prerender.mjs`, `app/entry-server.tsx`) into `<route>.html`, served by
  `src/api/apps/auth.rs` (same list) for `/auth/<route>`; `app/main.tsx` hydrates them.
  Keep the first render of hydrated routes free of URL/storage reads
  (`lib/hydration.ts` `useHydrated`).
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

Logout (`src/api/oidc/logout.rs`, RP-Initiated Logout 1.0; `iam/oidc/backchannel_logout.rs`,
Back-Channel Logout 1.0):
- `/oauth/logout` takes `id_token_hint` (may be expired), `client_id`,
  `post_logout_redirect_uri` (exact match against the client's
  `post_logout_redirect_uris`), and `state`. Invalid hint, unregistered or client-less
  redirect, or unknown client -> `400`, no redirect, session kept. A cross-site `POST`
  (no `SameSite=Lax` session cookie) is bounced to the `GET` form.
- A hint naming the browser's current session (user and `sid`) ends it at once.
  Otherwise the request is parked (`oauth:logout-request:{id}`, 10 min, id in the
  `oauth_logout` cookie, `SameSite=Strict`, `Path=/oauth/logout`) and the user confirms
  on the prerendered `/auth/logout` page, whose form posts natively to
  `/oauth/logout/confirm` (`action=logout|cancel`). Then `post_logout_redirect_uri`
  (+`state`) or `/auth/signed-out`; the session cookie is cleared.
- Ending a session notifies each linked client with a `backchannel_logout_uri`: a
  `logout_token` (`typ: logout+jwt`, ID token key, `sub`, `aud`, `sid`, `events`, 2 min)
  posted in-process, best effort (5s timeout, 2 retries on network errors/5xx, none on
  4xx, no redirects, not persisted). RP-Initiated Logout waits up to 3s before redirecting.
  `OAUTH_CLIENT_CA_FILE` adds trusted roots. Expiring sessions are not announced.
- The Console logs out through `/oauth/logout` with its ID token; `admin bootstrap`
  registers the Console root (callback URI without `/auth/callback`) as its
  `post_logout_redirect_uri`.

Authorization request handling:
- Errors after the client and `redirect_uri` are verified are redirected with
  OAuth codes (`invalid_request`, `unsupported_response_type`, `unauthorized_client`,
  `invalid_scope`, `login_required`, `consent_required`, `interaction_required`,
  `request_not_supported`, `request_uri_not_supported`); before that, `400`.
- `response_mode`: `query` (default) or `form_post` (auto-submitting HTML form,
  `no-store`); errors and consent denial use it too, anything else is `invalid_request`
  in the query. A rejected request object answers in the `response_mode` / `state` it
  carries (`crates/oidc/src/request_object.rs`, read unverified).
- Unregistered scopes are dropped; `openid` is required. `max_age=0` = `prompt=login`.
- `id_token_hint` must verify (else `invalid_request`); a session of another user is
  `login_required` with `prompt=none` or after a resumed sign-in, else the login page
  prefilled with the hinted user's email.
- `acr_values` (`crates/oidc/src/authentication.rs`): two levels,
  `urn:stargate:acr:single-factor` / `urn:stargate:acr:multi-factor`, from the session's
  `amr`; the highest requested level the session reaches is issued, unknown values are
  ignored. If none is reached: park with a `StepUpStage` and send the browser to
  `/auth/step-up?interaction={id}` (JS only; email code via the session MFA API), or to
  the login page for an email-code-only session; `prompt=none` -> `interaction_required`;
  MFA unavailable (`email_mfa_available`) or a resumed email-code sign-in -> the actual level.
- A replayed authorization code revokes the access token and refresh family it issued.

Signing keys (`src/iam/auth/jwt_keyring.rs`, `crates/jwt/src/keyring.rs`):
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
- ID tokens and OAuth access tokens carry `acr` and `amr`; introspection adds them with
  `auth_time`; codes and refresh families keep the grant's `Authentication`.

Provider state:
- SQL: `oauth_clients`, `oauth_consents`.
- Store: `oauth:authorization-code:{code_hash}` (pending/redeemed, TTL),
  `oauth:refresh-family:{family_id}` (CAS rotation), `oauth:authorize-interaction:{id}`,
  `oauth:logout-request:{id}`, `oauth:client-assertion:{client_id}:{jti_hash}` (assertion
  replay, until `exp`), and JWT `jti` revocations until expiry.

Client notes:
- Client secrets are returned only on create/rotate and stored as hashes.
- Logout metadata are columns (`post_logout_redirect_uris`, same rules as
  `redirect_uris`; `backchannel_logout_uri`, absolute HTTPS or loopback HTTP, no
  fragment; `backchannel_logout_session_required`, informational since `sid` is always sent).
- Public clients use `token_endpoint_auth_method = none` and always need PKCE.
- Client authentication (`src/api/oidc/client_auth.rs`, token/introspect/revoke): one
  method per request, the registered one. `private_key_jwt` clients register exactly
  one of `jwks` (public RSA/EC keys only) or `jwks_uri` (columns) and have no secret.
  Assertions (`crates/oidc/src/client_assertion.rs`, `jwt::decode_with_jwk_set`):
  asymmetric algs, `iss = sub = client_id`, `aud` the issuer, the token endpoint or the
  called endpoint, `exp` <= 1h ahead, `jti` required and single-use. `jwks_uri` sets are
  cached 5 min and refetched on an unknown key at most every 10s (`iam/oidc/client_jwks.rs`).
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

Conformance: Config OP, Basic OP, Form Post OP, RP-Initiated Logout OP, and Back-Channel
Logout OP plans, plus the comprehensive `oidcc-test-plan` with `private_key_jwt`, run in
`.github/workflows/oidc-conformance.yml` (builds `apps/auth` too). `op-setup.sh` writes
`op.json` (Basic, Form Post, RP-Initiated), `--backchannel` and `--private-key-jwt`
configs (`client-jwk.mjs` keys); only the back-channel client registers a
`backchannel_logout_uri`, since an unexpected back-channel post fails the RP-Initiated
tests. The suite is served with a test-CA certificate that Stargate trusts
(`OAUTH_CLIENT_CA_FILE`). Expected warnings/skips/failures are in `.github/oidc-conformance/`;
the runner fails on expected results no module produced, so the comprehensive plan has its
own `private-key-jwt-expected-failures.json` (adds `oidcc-server-rotate-keys`, which needs a
live key rotation).
Docs: `docs/oauth-oidc-provider-guide.md`, `docs/oidc-conformance.md`.

## OTP, Passwordless Login, and MFA

Source layout:
- `crates/otp`: HOTP (RFC 4226), TOTP (RFC 6238), Base32, provisioning URIs,
  short-lived message OTP records for email/SMS.
- `src/iam/otp`: policy evaluation, challenge persistence, verification, delivery,
  throttling, service flows. Workflow changes go here. `settings.rs` reads and validates
  the `MFA_*` / `EMAIL_OTP_*` variables once at startup (`otp_settings()`; strict values,
  `sms`/`totp` rejected); `EMAIL_OTP_PEPPER` is required when MFA is mandatory for someone
  (`MFA_MODE=required`, `MFA_REQUIRED_FOR_SUPER_ADMIN=true` in optional mode,
  `MFA_ADMIN_STEP_UP_REQUIRED=true`), else a startup warning if users can opt in.
- `src/api/account/otp`: thin Axum/OpenAPI bindings.

Account endpoints:
- `POST /account/login/otp/email` - start passwordless email OTP (generic response for unknown users).
- `PUT /account/login/otp/email` - verify and issue a session when MFA policy allows passwordless.
  Both need `PASSWORDLESS_LOGIN_ENABLED=true` (default off: `404 feature.disabled`); turning
  it on needs `EMAIL_OTP_PEPPER` and stops startup with `MFA_MODE=required`.
- `PUT /account/login/mfa/challenges/{challenge_id}` - complete login MFA.
- `GET /account/mfa/methods` - effective MFA policy/methods.
- `POST /account/mfa/challenges` - authenticated challenge (email only).
- `PUT /account/mfa/challenges/{challenge_id}` - verify and store a step-up marker; also
  adds the factor to the session (`amr`), which the OIDC step-up relies on.

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

Per-recipient email throttle (`src/iam/email_throttle.rs`): unauthenticated flows
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

Account creation (`SIGNUP_MODE`, `src/conf/features.rs`; administrators can always use
`POST /admin/users`):
- `open`: self-registration (`POST /signup`) and accounts created by a first social
  sign-in, limited to `SIGNUP_ALLOWED_DOMAINS` when set (exact domains; rejected with
  other modes; `403 signup.domain_not_allowed`).
- `invite` (default): admin invitations only; `POST /signup` is `404 feature.disabled`,
  and social sign-in works for existing accounts only (`403 social.account_not_provisioned`).
- `closed`: no invitations either; `/admin/users/invitations` and `GET`/`PUT /signup` are
  `404 feature.disabled`.
- Pending signups record their `origin` (`self_service` / `invitation`,
  `iam::user::signup::SignupOrigin`): a self-registration completes only while the mode is
  still `open` and its domain allowed (`api::signup::ensure_completable`), so a stricter
  mode applies to signups under way.

Signup (`POST /signup`, `open` only): always `200 signup.requested` for a valid address
(`400 request.invalid` otherwise). In the background a new address gets the
verification link, a pending signup has its link resent (same `sid`), and an
existing account gets a `SignupAccountExists` sign-in reminder, so neither the
response nor its timing reveals the account. Admin invitations
(`send_invitation`) keep `409` conflicts and wait for delivery.

Emails: templates come from `apps/mail/emails/*.tsx` (`npm run export` writes
`.stargate/transactional/*.html`). Action URLs are built by the server
(`src/web/links.rs`: `signup_url`, `change_password_url`, `login_url`, based on
`AUTH_PUBLIC_BASE_URL`, else the public base URL plus the auth app base path) and
rendered as `{{link}}`; `{{name}}` is the only other placeholder.

## Database Schema (migrations/)

Tables: `organizations`, `users`, `super_admins`, `credentials`, `credential_history`,
`api_keys`, `admin_keys`, `service_accounts`, `oauth_clients`, `oauth_consents`,
`user_organizations`, `user_api_keys`, `service_account_api_keys`, `outbox_events`,
`config_documents` (append-only gateway configuration and ACE policy versions).
The initial migration per backend (`migrations/{postgres,sqlite}/..._init_schema.sql`)
creates the base schema; later forward migrations extend it.

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
  `sub_id`. API writes pre-check with
  `iam::user::nicknames::ensure_nickname_available` (`400 user.nickname_invalid`,
  `409 user.nickname_already_exists`).
- Social sign-in creates accounts only with `SIGNUP_MODE=open` (see "Passwords").
- Social sign-in matches and creates accounts by an email the provider verified, else
  `403 social.email_unverified`: GitHub's primary verified email from `/user/emails`
  (scope `user:email`), Google's `userinfo` email with `verified_email` (scope `email`,
  bearer access token). `crates/idp`: `GitHubUser` / `GoogleUser`.
- A first GitHub sign-in takes the GitHub username as nickname, or the first free of
  `<login>_2` ... `_9`, then `<login>_<random>`, then the email
  (`iam::user::nicknames::available_nickname`; GitHub usernames have no `_`). A nickname
  taken between the check and the insert is retried (`src/api/social/github.rs`).
Store keys derived from emails use the canonical form too.

IDs are ULIDs stored as text. `outbox_events` holds the immutable JSON payload,
`seq`, optional `operation_id`, `created_at`, and nullable `published_at`, with
the same logical columns on PostgreSQL and SQLite.

## Key Env Vars

| Var | Default | Purpose |
|-----|---------|---------|
| `CONFIG_PATH` | `.stargate` | Directory for gateway/policy seed files; also holds other runtime assets |
| `CONFIG_FILENAME` | `config.yaml` | Gateway seed filename, used only when its database document is absent |
| `CONFIG_SYNC_INTERVAL_SECS` | 30 | Gateway/policy reconciliation interval in both profiles (integer >= 1); validated at startup |
| `PORT` | 5050 | HTTP listen port |
| `STARGATE_RUNTIME_PROFILE` | - | `edge` or `cluster` (validated against the build) |
| `JWT_ALGORITHM` | active key / `RS256` | Must match the active key when set; picks the first generated key |
| `JWT_SECRET` | - | HMAC key for HS* algorithms |
| `JWT_ISSUER` | `http://localhost:$PORT` | JWT and OAuth/OIDC issuer |
| `JWT_KID` | `stargate-current` | Active signing key (`.stargate/jwks/keys/<kid>/`) |
| `JWT_ID_TOKEN_KID` | - | Optional RS256 key that signs ID tokens (EC or HMAC active key) |
| `JWT_ACCESS_EXP` / `JWT_REFRESH_EXP` | 1h / 1d | Token TTLs |
| `OAUTH_BASE_URL` | issuer | Public base URL (`conf::public_base_url`): provider endpoints, email links, OpenAPI `servers` |
| `IAM_HOSTS` | all hosts | Comma-separated IAM hostnames, case-insensitive, without schemes, ports, paths, or wildcards; must include the public base URL host |
| `JWKS_CACHE_MAX_AGE_SECS` | 300 | JWKS cache max-age |
| `OAUTH_CLIENT_CA_FILE` | - | PEM roots trusted (with the system ones) for OAuth client endpoints (back-channel logout, `jwks_uri`) |
| `INTERNAL_CONTEXT_*` | see `.env.example` | Internal-context issuer, kid, key/JWKS paths, TTL, skew, signing workers/queue |
| `POSTGRES_ENDPOINT` | localhost:5432 | Cluster DB endpoint: host, host:port, [ipv6], or [ipv6]:port; invalid endpoints stop startup |
| `POSTGRES_USERNAME` / `_PASSWORD` / `_DATABASE` | root / root / stargate | PostgreSQL fields passed directly to sqlx; no URL encoding |
| `POSTGRES_SSLMODE` | prefer | disable, prefer, require, verify-ca, or verify-full; other values stop startup |
| `POSTGRES_SSLROOTCERT` | - | Private root CA PEM path for PostgreSQL TLS |
| `POSTGRES_CREATE_DATABASE` | true | true/false; false skips maintenance DB access and CREATE DATABASE; migrations still run |
| `DB_POOL_MAX` | 20 | Maximum connections in each SQL pool |
| `REDIS_URL` | - | Redis (cluster) |
| `RUST_LOG` | - | Tracing/log filter directives |
| `LOG_FORMAT` | pretty (debug), json (release) | Console format: json, compact, or pretty; invalid values stop logging initialization |
| `LOG_CONSOLE_ENABLED` | true | Log to stdout; stays enabled if both sinks are disabled |
| `LOG_FILE_ENABLED` | false | Enable optional JSON log files |
| `LOG_DIR` | .stargate/logs | Directory used only when file logging is enabled |
| `OTEL_ENABLED` | auto | OTLP export; on when an endpoint is set |
| `OTEL_EXPORTER_OTLP_ENDPOINT` / `_PROTOCOL` | - / grpc | OTLP target (per-signal overrides exist) |
| `AUDIT_RELAY_ENABLED` | true | Run the outbox relay (cluster) |
| `AUDIT_RELAY_BATCH` / `AUDIT_RELAY_INTERVAL_MS` | 256 / 1000 | Relay batch size / idle poll |
| `AUDIT_REDIS_STREAM` | audit.raw | Redis stream for raw audit events |
| `AUDIT_STREAM_MAXLEN` | 100000 | Approx stream cap (`MAXLEN ~`; 0 = unbounded) |
| `TLS_ENABLED`, `TLS_CERT`, `TLS_KEY` | false | Listener TLS; with it off, an `https://` public base URL still marks the deployment HTTPS |
| `TRUSTED_PROXIES` | - | Proxy IP/CIDRs allowed to set client IP headers |
| `CORS_ORIGINS` | - | Allowed CORS origins |
| `TRUSTED_ORIGINS` | - | Trusted browser origins for sensitive IAM endpoints |
| `GEOIP_DB_PATH` | - | MaxMind GeoLite2 |
| `SERVER_DRAIN_DELAY_SECS` | 5 | Readiness-off delay before listeners close |
| `SERVER_SHUTDOWN_TIMEOUT_SECS` | 25 | Connection drain deadline |
| `AUTH_APP_DIR` / `AUTH_APP_BASE_PATH` | `.stargate/apps/auth` / `/auth` | Hosted auth app files and mount (empty or `/` = default) |
| `AUTH_PUBLIC_BASE_URL` | - | Public auth app URL when hosted elsewhere |
| `CONSOLE_APP_DIR` / `CONSOLE_APP_BASE_PATH` | `.stargate/apps/console` / `/stargate` | Console files and mount (empty or `/` = default) |
| `SIGNUP_MODE` / `SIGNUP_ALLOWED_DOMAINS` | invite / - | Who may create accounts (`open`, `invite`, `closed`); domains for `open` |
| `PASSWORDLESS_LOGIN_ENABLED` | false | Passwordless email-code login (needs `EMAIL_OTP_PEPPER`; not with `MFA_MODE=required`) |
| `CONSOLE_ENABLED` | true | Serve the Console app (`false`: its path answers `404 feature.disabled`) |
| `CONSOLE_OAUTH_CLIENT_ID` | stargate_console | Reserved Console OAuth client |
| `CONSOLE_OAUTH_REDIRECT_URI`, `CONSOLE_PUBLIC_URL` | derived | Console callback / origin |
| `EMAIL_OTP_PEPPER`, `EMAIL_OTP_LENGTH`, `EMAIL_OTP_TTL_SECS`, `EMAIL_OTP_MAX_ATTEMPTS` | -, 6, 300, 5 | Email OTP |
| `MFA_MODE` | optional | `off`, `optional`, `required` |
| `MFA_METHODS` / `MFA_DEFAULT_METHOD` / `MFA_EMAIL_ENABLED` | email / email / true | MFA methods |
| `MFA_REQUIRED_FOR_SUPER_ADMIN` | false | MFA for super-admins in `optional` mode |
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

# Back up or import exact database document content (both profiles)
cargo run --features edge -- config export --kind gateway --output gateway-backup.yaml
cargo run --features edge -- config export --kind policies --output policies-backup
cargo run --features edge -- config import --kind gateway --file gateway.yaml
cargo run --features edge -- config import --kind policies --file policies

# Next JWT signing key in .stargate/jwks/keys/<kid>/;
# --id-token creates the RS256 key for JWT_ID_TOKEN_KID
cargo run --features edge -- jwt generate-key --kid stargate-2026-11

# Internal-context signing key + public JWKS (refuses to overwrite)
cargo run --features edge -- internal-context generate-key \
  --output-dir .stargate/internal-context --kid stargate-internal-current
```

Bootstrap flags: `--password`, `--password-stdin`, `--generate-password`,
`--oauth-client-id`, `--oauth-redirect-uri`. `jwt` and `internal-context` commands
need no database. Configuration commands require the database, keep stdout free of
banners/logs, and validate syntax/compilation without local TLS or signer material.
Imports preserve the submitted text, retry a revision conflict once, and write an
atomic `cli_import` system audit event for changed content. Runtime replicas do full
preparation before activation. The Helm ConfigMap is a seed by default; cluster-only
`gatewayConfig.importOnUpgrade` imports both documents on install/upgrade and replaces
Console edits. Edge imports use `kubectl exec` in the existing pod. Docs: `docs/cli.md`.

## API Overview

- `GET /livez` (process liveness), `GET /readyz` (initialized, not draining, SQL and
  cluster Redis up). See `docs/health-probes.md`.
- `GET /docs` (OpenAPI JSON), `GET /docs/errors` (error catalog), `GET /i18n/{locale}` (`en`, `it`)
- `/.well-known/*`: OAuth/OIDC metadata, JWKS, `stargate-context-jwks.json`
- `/account/*`: login, logout, profile, refresh, credentials, organizations, consents, OTP/MFA,
  `features` (public: available sign-in options)
- `/oauth/*`: provider endpoints; `/oauth/state`, `/oauth/github`, `/oauth/google`: social login
- `/signup/*`: registration + email verification
- `/admin/*`: users (+ invitations), orgs, API/admin keys, OAuth clients, service accounts,
  config, access-control rules, sessions, outbox events, `me`, `overview`, `health`.
  Super-admin users bypass capability checks; admin keys need exact `<resource>:<action>`
  permissions (`src/api/admin/authorization.rs`).
- `/auth/*` (hosted auth app), `/stargate/*` (console), everything else: gateway fallback.

`IAM_HOSTS` optionally restricts every IAM route above (including docs and hosted
apps) to exact hostnames. Unset or empty preserves IAM routes on every host.
Entries are validated once at startup, compared in lowercase without the request
port, and must include the host of `conf::public_base_url()` (`OAUTH_BASE_URL`, else
the issuer). Schemes, ports, paths, and wildcards in entries stop startup.

`api::server_router` mounts probes on every host, including pod IPs and hostless
requests. Its `dispatch::HostDispatch` sends allowed hosts through the IAM router
and its ingress limit, with the gateway as fallback. Other hosts and hostless
requests go directly to the gateway when `IAM_HOSTS` is configured, where the
gateway ingress limit still applies. `main.rs` adds security headers, tracing,
CORS, compression, and normalize-path to all requests.

IAM limits (`src/api/middleware/rate_limit.rs`): every IAM route charges the client IP's
ingress allowance, the same `ingress:ip:{ip}` bucket as the gateway, before
credentials are read (`429 iam.ingress_rate_limit_exceeded`). Authenticated
`/admin/*` principals then charge their own `default` bucket, keyed
`lim:admin:user:{id}` / `lim:admin:key:{id}` in `extract_admin_authorization`
(`429 iam.rate_limit_exceeded`, scope `subject`), so one admin key cannot throttle
the console or other keys behind the same IP. Limiter failures: `503 iam.limiter_unavailable`.
Login brute force is handled separately by `iam/auth/login_guard.rs` and OTP throttles.

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
(`OAuthErrorResponse`, `OAuthResult`, and the `oauth_*` constructors in
`src/err/oauth.rs`).

Statuses in use: 400, 401, 403, 404, 405, 408, 409, 413, 415, 429, 500, 502, 503, 504.

## Rate Limit Response Headers

`x-ratelimit-limit`, `x-ratelimit-remaining`, `x-ratelimit-scope`, `x-quota-limit`,
`x-quota-remaining`, `x-quota-scope`, `retry-after`. Scope is `subject`, `org`, `ip`,
or `ingress`. `429` `retry-after` is whole seconds rounded up (missing duration
-> 60); `503` uses 10. Shared helpers: `ingress_key` in `src/gate/limits.rs`,
`limited_response` and `insert_limit_headers` in `src/web/limits.rs`.

## Graceful Shutdown

SIGTERM/SIGINT -> readiness off -> wait `SERVER_DRAIN_DELAY_SECS` -> gateway admission
closes and active work is cancelled -> listeners close and connections drain
(`SERVER_SHUTDOWN_TIMEOUT_SECS`) -> tracked mirror/WebSocket tasks drain -> stop configuration synchronization -> stop
audit relay -> save in-memory store -> flush telemetry.

## Frontend (apps/)

`apps/` holds the TypeScript frontends, separate from the Rust binary:

- `console`: super-admin console (users, orgs, API/admin keys, OAuth clients,
  service accounts, sessions, audit outbox, gateway config, access-control
  policies). Reference implementation of the layering below. Served at `/stargate`.
- `auth`: hosted login / auth-flow UI (password login, passwordless email OTP,
  login MFA, reset/change password, signup request and completion, organization
  choice, OIDC consent and step-up, logout confirmation and signed-out pages). Same
  stack and layering. Served at `/auth`; its login, logout, and signed-out routes are
  prerendered and hydrated (see "Provider login"), other routes are client-rendered
  from `index.html`. The layout loads `GET /account/features` after hydration (store
  `features`, null before, so prerendered markup still matches): the login page shows
  the passwordless link, "Continue with Google/GitHub" for the configured providers
  (`lib/env.ts` `socialSignInUrl`), and, in `open` signup mode, a sign-up link; `/signup` without a
  token is the request form (`signup-request.tsx`), or explains that accounts come from
  an administrator.
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
`/admin/overview` plus recent outbox events for the dashboard. The health card also
shows the server's `features` (signup mode, passwordless, social providers); the Users
page reads them too and offers invitations unless the signup mode is `closed`.

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
- Gateway performance workloads are opt-in (`src/gate/performance/`,
  `benches/scripts/`).
