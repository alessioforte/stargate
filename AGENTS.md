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
│   ├── cli/           # CLI: admin bootstrap, db migrate, config export/import, internal-context / jwt generate-key
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
│   ├── smtp/          # Email delivery (lettre with rustls TLS)
│   ├── store/         # State store (DashMap / Redis)
│   └── tools/         # Shared duration parsing and hostname validation
├── apps/              # TypeScript frontends (see "Frontend")
│   ├── auth/          # Hosted login / auth-flow UI, served at /auth
│   ├── console/       # Super-admin console, served at /stargate
│   └── mail/          # Transactional email templates (react-email, build-time only)
├── migrations/        # sqlx migrations split by backend: postgres/, sqlite/
├── docs/              # Guides: config-v1, features, oauth-oidc-provider, oidc-conformance, multi-org,
│                      #   password-policies, health-probes, listeners, internal-context-*, cli, docker, api errors,
│                      #   gateway-performance (opt-in benchmarks and recorded results),
│                      #   redis-keys, redis-failure-bench, production-checklist, audit-events, monitoring, sizing
├── k8s/               # Kubernetes manifests
└── benches/           # Criterion benchmarks; scripts/ has gateway, backend and Redis failure runners
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
- **Auth**: JWT (jsonwebtoken on its aws-lc-rs backend), Argon2 passwords, RSA/EC/HMAC signing
- **Serialization**: serde_json, serde-saphyr, MessagePack
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

- `conf/`: strict env readers (`env.rs`), the edge data directory (`data.rs`, `DATA_PATH`,
  default `.stargate`, for `sqlite.db` and `memory.bin`), the config directory (`config_dir`,
  `CONFIG_PATH`), the public base URL (`public_base_url`: `OAUTH_BASE_URL`, else the
  issuer; never the listening host), the hosted apps' base paths (`apps.rs`, read by the app routers,
  links, and the Console's default callback), IAM host selection (`hosts.rs`,
  `IAM_HOSTS`, validated once at startup), and feature toggles (`features.rs`, below).
  Tests of process-wide state rerun one test of the binary through
  `conf::child_test::run`, which starts the child in its own empty working directory
  (never the parent's `.stargate`, so no shared JWT keyring).
- `ctx/`: request metadata and ingress headers (`request.rs`), lazy GeoIP; signed-context
  drafts (`propagation.rs`) and audit/authorization projections (`adapters.rs`).
  `ctx/sign/`: internal-context signer runtime, `settings.rs`, `material.rs` key/JWKS
  validation, `signing.rs` bounded CPU workers.
- `data/db/`: SQL pool init, PostgreSQL endpoint/TLS/creation settings (`settings.rs`),
  shared active-document/seed persistence (`config.rs`), and the repository service
  wrappers (`svc.rs`), and SQL pool observables (`metrics.rs`). Seed closures own file and validation rules. Revision queries
  return typed configuration kinds; unknown database kinds are errors.
  PostgreSQL fields go directly into
  `PgConnectOptions`, including unencoded credentials. Optional database creation
  uses the same credentials/TLS through the `postgres` maintenance database;
  concurrent-create errors are benign only for `42P04`, or `23505` on
  `pg_database_datname_index`, after a fresh existence query confirms the target.
  `data/store/`: selected profile interface; `memory.rs` persistence (backup at
  `DATA_PATH/memory.bin`, next to the edge SQLite database;
  `STORE_MAX_MEMORY_MB` validated at startup), `redis.rs` init/readiness and
  Redis TLS/credential settings (`settings.rs`). Redis uses verified TLS for
  `rediss://`, public roots by default or `REDIS_CA_FILE`, and
  separate credentials without URL encoding. The store, limiter, and audit relay
  share the same pool; URL credentials cannot be combined with separate fields.
  `crates/store/src/keys.rs` owns every state-key name:
  `stargate:v1:{<entity>}:<name>[:<qualifier>...]`. The version is part of the
  root, every key carries one entity hash tag (co-accessed keys share it), and
  batch operations split by tag so a Redis Cluster slot never mixes entities.
  The pool opens connections on demand and requires `COMMAND INFO HEXPIRE`
  to succeed on every socket (Redis 7.4+ or Valkey 9.0+). An unavailable or
  unsupported backend does not block startup. The security-state request budget
  (`REDIS_SECURITY_TIMEOUT_MS`, default 1000) includes ingress and all security
  store waits. Store outages yield `503 auth.state_unavailable` (OAuth keeps its
  error envelope), with `Retry-After: 10`, for session, revocation, API key,
  OAuth state, OTP/MFA, password-reset and signup state; conflicts and other
  internal failures stay `500`. Optional replication confirmation
  uses a separate pool connection and a plain pipeline containing the security
  write and `WAIT`. It covers session logout/revocation, token revocation and
  revoked API key cache deletion. The default replica target is zero.
  `REDIS_KEY_PREFIX` (empty by default) is prepended literally before that root,
  including batch/atomic operations and limiter Lua keys. Hash fields are
  unchanged; audit streams use the explicit `AUDIT_REDIS_STREAM` name without the
  schema or prefix. Changing the schema version or the prefix hides existing
  sessions, revocations and counters; coordinate migration/draining across
  replicas. Guide: `docs/redis-keys.md`.
  A configured `REDIS_CA_FILE` requires `rediss://`; a cleartext URL fails startup.
- `obs/`: logging (`logging/settings.rs` parses the format and sinks), and the
  telemetry provider (`telemetry/metrics.rs`, `telemetry/propagation.rs`). OTLP
  HTTP export uses a blocking client because SDK batch workers run outside Tokio.
  Logs go to stdout by default; `LOG_FORMAT` defaults to pretty in debug and JSON in
  release. Optional files remain JSON. ANSI requires a stdout terminal and a text
  format. The server validates settings before its banner and omits it in JSON
  mode; CLI output keeps its existing behavior. Server spans follow HTTP semantic
  conventions with matched route names and standard attributes; they omit query
  strings entirely. Duration histograms use seconds and explicit boundaries.
  JSON includes the current span
  and span list: request logs retain `request_id`, including in nested spans, and
  record the valid SDK `trace_id` when OpenTelemetry is active. With it disabled,
  `trace_id` stays absent; no synthetic trace id is generated. All three OTLP signals
  share resource attributes read by the SDK from `OTEL_RESOURCE_ATTRIBUTES`.
  `service.instance.id` defaults to a process-lifetime ULID when not explicitly set;
  the chart sets it to the pod name and also supplies pod/namespace attributes
  through ordered downward-API environment entries. The chart rejects duplicate
  `POD_NAME`, `POD_NAMESPACE` and `OTEL_RESOURCE_ATTRIBUTES` settings.
- `srv/`: the listener manager (`mod.rs`) with its validated settings
  (`settings.rs`), listener TLS (`tls.rs`), health lifecycle/probes (`health.rs`),
  profile validation, startup banner, shutdown signal and drain (`shutdown.rs`).
  `LISTENERS` declares named listeners with an address, planes, a TLS switch, and a
  connection cap; without it one implicit listener serves every plane on
  `0.0.0.0:$PORT` (see "Listeners"). All listeners bind before any serves, every
  connection gets hyper timeouts, HTTP/2 limits, `TCP_NODELAY`, and a slot from its
  listener's cap, and a failed accept backs off. Each request carries
  `srv::ConnectionInfo` (peer and the listener's name, planes, TLS, proxy trust): read
  connection facts from it instead of a process-wide setting.
  `srv::client_ip` resolves the client address once at ingress from the peer and that
  listener's `ProxyTrust` (`srv::proxy`), removes `Forwarded`, `X-Real-IP`, and every
  `X-Forwarded-*` field from an untrusted peer on every plane, and stores
  `srv::client_ip::ClientIp` in the request extensions. Logging and spans, ingress and
  admin limits, login guards, OTP, source-IP matchers, audit, and internal-context
  signing all read it from there.
  `web::request::RequestExt::is_https` decides HTTPS per request through
  `tls::request_https(listener_tls)`: the receiving listener's TLS or the public base
  URL scheme (read once). It drives the `Secure` cookie attribute, HSTS, the request's
  own origin scheme, and the span scheme; `tls::over_https` is the pure test helper.
- `web/`: request extraction (`request.rs`), origins/CORS (`origin.rs`
  trusted-origin middleware, `cors.rs`; both read and validated once at startup),
  security and retry headers, the session cookie
  (`cookies.rs`), base and auth-app URLs (`links.rs`), limit headers and `429` answers
  (`limits.rs`), message envelopes.
  `cookies::SESSION_COOKIE` is the shared session-cookie name for reading,
  creation and upstream sanitization.
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
  Guide: `docs/features.md`.

### Listeners

`srv::settings` reads and validates `LISTENERS` and, for each declared name,
`LISTENER_<NAME>_ADDRESS`, `_PLANES`, `_TLS`, `_MAX_CONNECTIONS`, and
`_TRUSTED_PROXIES` once at startup. Invalid or duplicate names, shared addresses,
incomplete addresses, unknown planes, a management plane sharing its listener, TLS
on a management listener, `PORT`/`TLS_ENABLED`/`MAX_CONNECTIONS`/`TRUSTED_PROXIES`
alongside `LISTENERS`, an invalid proxy entry, an unknown `LISTENER_*` variable, and
a declared configuration without `JWT_ISSUER` all stop it.
Without `LISTENERS` the implicit listener is `0.0.0.0:$PORT` with every plane,
`TLS_ENABLED`, `MAX_CONNECTIONS`, and its own `TRUSTED_PROXIES`. Names are 1–32
ASCII letters or digits starting with a letter; addresses are literal IPs with a
port, IPv6 included.

Proxy trust is per listener and empty by default: only a peer in
`LISTENER_<NAME>_TRUSTED_PROXIES` (the implicit listener: `TRUSTED_PROXIES`) may set
forwarding headers. Trust is a list of sources (`srv::proxy::ProxyTrust`; today only
addresses and CIDR blocks, so a proxy certificate can join later). The same plane on
two listeners keeps two trust boundaries: an internal listener ignores forged
`X-Forwarded-For` while the ingress listener accepts it.

`main.rs` builds one router per listener with `api::listener_router` and applies
the gateway extension, security headers, and tracing once, never per connection;
`srv::serve` binds them all, logs each one, and runs the accept loops. TLS
listeners share `TLS_CERT` and `TLS_KEY` and offer ALPN `h2` and `http/1.1`.
Per connection: a 10 s first-request and header-read timeout (it also closes a
mute connection, a client that stops after the HTTP/2 preface, and an idle HTTP/1
keep-alive connection), a 10 s TLS handshake timeout, 100 HTTP/2 concurrent
streams, a 32 KiB HTTP/2 header list, a 30 s keep-alive PING with a 10 s timeout,
`TCP_NODELAY`, and a slot from the listener's connection cap (default 1024); a
failed accept backs off 100 ms. Upgraded WebSockets leave the cap and are bounded
by gateway admission, and a proxy that reuses connections needs an idle timeout
below 10 s (Traefik: `serversTransport.forwardingTimeouts.idleConnTimeout`).
Every request carries `srv::ConnectionInfo` (peer, listener name, planes, TLS,
proxy trust) and the resolved `ClientIp`; handlers read the client address from
`RequestExt` and never extract `ConnectInfo`. Tests that build a request
themselves insert `ConnectionInfo` (and run `srv::client_ip::prepare` when they
exercise forwarding headers). Guide: `docs/listeners.md`.

### Gateway Runtime (`src/gate/`)

`gate::init` builds the runtime; `gate::service` is mounted as the Axum
`fallback_service`. Request flow:

1. Pin the active `RuntimeSnapshot` (graph, balancers, limiter, prepared clients,
   version) and the independent `PolicySnapshot` (ACE engine + revision); acquire
   process admission (`503 gateway.overloaded` when exhausted).
2. Charge the mandatory `ingress.limit` IP bucket with the client address the
   server resolved once at ingress, bounded by `ingress.timeout` (default 250ms):
   `429` on denial; store errors/timeouts pass by default (`on_store_error: allow`),
   or return `503` with `deny`, before any credential check or route match.
   The listener already removed `Forwarded`, `X-Real-IP`, and every `X-Forwarded-*`
   field from an untrusted peer before routing. Capture the original peer and host;
   operator middleware may then supply forwarding metadata.
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
`metrics.rs` (current-snapshot target diagnostics), `resources.rs` (process admission, memory permits, tracked tasks), `transport/`
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
  Org-scoped limits have `on_missing: skip | ip_fallback | deny`. Quota windows
  up to an hour stay in the state store; day, week, month and year keep a
  durable balance in SQL (`timezone` required, `anchor_day` for a billing
  month) and are always fail-closed. A long quota belongs to an authenticated
  subject or an organization: compilation rejects it with `ip_fallback` or, at
  subject scope, on a router without an `auth` policy. `lim::DurableQuotas`
  keeps a replica's reserved batches across generations of the same named quota.
  Each bucket retains its charged units and window bounds when its calendar
  changes. The new calendar starts at the stored end. Each named quota's
  activated definition controls SQL reservations. Saving a configuration for
  restart leaves the definitions active. A snapshot with a retired definition
  can consume previously reserved units and fails closed on new reservations.
- Upstreams: `transport.connect_timeout`, `transport.protocols` (`http1`/`http2`;
  WebSocket needs `http1`), `load_balancer.liveness_probe` and `circuit_breaker`
  (`fail_threshold` 3, `cooldown` 30s), optional `internal_context.audience`.
  HTTP and WebSocket connectors set `TCP_NODELAY`. HTTP pools retain up to
  32 idle sockets per origin with a 30s idle timeout. The regular HTTP/2 client sends PINGs
  after 20s without traffic, including on idle connections, with a 5s ACK deadline.
  With a stream open, the dedicated gRPC client sends a PING after 300s without
  traffic and waits 20s for an ACK, including on receive-only streams. The pool
  closes idle connections without open streams before the first gRPC PING.
  The interval respects the default 5-minute minimum between data-free gRPC PINGs.
  Probes support HTTP and gRPC health checks; gRPC probes require HTTP/2.
  Both use a client prepared once per upstream with its TLS trust and client
  identity. The load balancer never creates a fallback probe client.
  `liveness_probe.timeout` defaults to 2s and must be positive and representable;
  it bounds connection establishment and the total probe request. HTTP probes
  use HTTP/2 prior knowledge and TLS ALPN `h2` when HTTP/1 is disabled.
  `forward_credentials` defaults to false, removing Authorization, API keys,
  the `jwt` cookie and token query parameters before dispatch. Other cookies
  use tolerant byte-based removal unless internal context requires strict parsing.
  Malformed query parameter names are retained without internal context; with
  internal context they are rejected as `400 request.invalid`. Credential
  forwarding travels with the selected upstream, separately from ingress facts.
  Opt in for a backend that validates the credentials;
  this cannot be combined with internal context. Proxy-Authorization and
  Connection-nominated fields are always removed as hop-by-hop headers.
- Routers: `response_mode: stream` for long-lived responses (default `finite`).
  Optional `timeouts.request` and `timeouts.upload_idle` override runtime values;
  `none` is allowed only for streams. gRPC requires an HTTP/2 upstream and sends
  the remaining `grpc-timeout` on every attempt; it returns gateway errors and
  stream failures as gRPC status, and records completed statuses.
- `runtime` defaults: primary 1024 / mirror 64 slots, replay 2 MiB per request /
  64 MiB total, upload idle 15s, connect 5s, response headers 30s, body idle 30s,
  mirror 5s, request 60s. Capacity fields require a restart.
- Ingress and rate/quota policies accept `on_store_error: allow | deny` (default
  `allow`). Ingress also governs IAM ingress and the admin principal limit.
  `ingress.timeout` is shared by all traffic checks for one request, including Redis
  connection waits. An exhausted check immediately follows its failure policy. Failed-open checks produce no allowance headers; configuration
  errors remain `500`. `gate/limits/store.rs` shares enforcement and process-lifetime
  degradation indicators across generations and application points. Indicators
  expire 60s after the last store error or clear on a subsequent successful check;
  `/admin/health` exposes the aggregate and a `limits.points` map. Expiry does not
  replace Redis diagnostics. Clock failures remain internal 500s, fail-closed.
  Organization override SQL failures always return `503 gateway.organization_limits_unavailable`,
  independently of the traffic store failure policy.
- Subject `attrs.rate_limit` / `attrs.quota` pick named limits; unknown names fail
  with `500 gateway.limit_configuration_invalid` (no silent fallback).

Full reference: `docs/config-v1.md`.

### Traffic Management

Implemented: weighted splits, mirrors (bounded disposal of discarded bodies),
failover on transport error, incompatible gRPC upstream protocol, and selected
statuses with per-service boundaries,
direct responses, catch-all fallback routes, replay buffering, per-upstream
protocol policy, finite/stream deadlines, WebSocket proxying, upstream mTLS,
signed internal context, gRPC over HTTP/2 with unary and streaming calls.

Partial: the circuit breaker (probes + live traffic; transport errors and
502/503/504 trip it, mirrors do not) is per-process, not shared across cluster
nodes, and its trip statuses are not configurable.

Missing: retry with backoff, request hedging, gRPC-Web, GraphQL-aware
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
System seeds, configuration imports and bootstrap use
`TrustedAuditContext::system_control_plane()`; credential rehash keeps its
application boundary.

- **Edge**: no relay; rows stay pending (`published_at = NULL`).
- **Cluster**: one relay per process (`src/aud/relay.rs`) claims pending rows in
  `seq` order with `FOR UPDATE SKIP LOCKED`, publishes to `AUDIT_REDIS_STREAM`
  (default `audit.raw`) as a single `payload` field, then marks them published and
  commits. At-least-once: consumers dedupe on `event_id`. Errors roll back and back off.
  With `AUDIT_STREAM_CONSUMER_GROUPS`, the relay
  trims only below the oldest protected ID across groups (pending first, otherwise
  last delivered), including while paused. XADD never trims in this mode; stream
  capacity pauses new claims, retaining the backlog in SQL. Without groups, MAXLEN
  can discard unarchived events. Cluster maintenance prunes only published outbox
  rows older than `AUDIT_OUTBOX_RETENTION` (default 30d), in batches with SKIP LOCKED.
  Cached audit diagnostics feed observable metrics, `/admin/health` and the Console,
  without changing readiness. Contract and consumer setup: `docs/audit-events.md`.

Super-admins read delivery state via `GET /admin/outbox-events[/{event_id}]` (read-only).

### Sessions

Each login has a stable JWT `sid`, shared by native access/refresh tokens and
linked user OAuth/OIDC tokens. A session's state shares the `{s:<sid>}` tag
(`store::keys::session`): the subject (`:subject`), the current native refresh
`jti` (`:refresh`) and the record (`:session`: owner, org, times, linked OAuth
client ids, `amr`). The per-user index `{u:<user_id>}:sessions` (sid → org,
expiry) names a user's sessions; no key lists every session.
User-delegated OAuth access tokens need a `sid` for a live session owned by the
token's `sub_id` on the admin API, profile, UserInfo, introspection, and gateway.
The record must be unexpired and the versioned subject under the same `sid` must
decode as a user with the record owner's ID (`load_active_session` reads both
concurrently; an unreadable record or subject reads as no session). Revocation
deletes a session's state even when its record is unreadable.
Client-credentials tokens are sessionless. OAuth refresh requires the family's
live session. Admin endpoints list and revoke sessions, and a native refresh-token
replay revokes the entire session.

`GET /admin/sessions` returns cursor pages (`iam/sess/listing.rs`): users come
from SQL in descending id order (only members with `organizationId`), sessions
from each user's index newest first, each checked like `load_active_session`. A
page examines at most 1000 users, so it can be short while `nextCursor` is set.

After an attribute, profile, or membership write commits, IAM rebuilds each active
session's stored `Subject` from SQL, keeping its org and reading the current role.
Registration reconciles the new subject before returning tokens. Each publication
reads the session subject revision, then reads attributes and the selected membership
in one SQL query. The store atomically replaces the observed value, increments its
revision and retains its expiration. A conflict retries both reads. A delayed write
cannot overwrite a later publication, and the SQL connection is free while Redis
is pending. If publication observes a missing membership, it revokes only the
session it read. Sessions opened after readmission keep their current role. Native refresh extends
the subject's TTL and OAuth refresh leaves its value and TTL alone. Post-commit
errors are logged. A failed update can leave the subject stale until a successful
update or the session ends.
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
Publication retains the active pools for upstreams with the same target URLs,
protocols, effective connect timeout, and loaded TLS material.
The Console preserves gateway and ACE drafts on conflicts, fetches the latest
document separately, and offers explicit Reload or Overwrite actions with its
version, update time and actor. ACE reads and writes expose the same metadata.
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

The Helm chart (`k8s/stargate`) can route through Traefik v3 `IngressRoute`
resources (`traefik.enabled`, mutually exclusive with the standard Ingress).
`values-traefik.yaml` is a three-replica cluster example
with ingress restricted to Traefik and explicit egress rules. The chart manages
`IAM_HOSTS` from the identity host plus explicit `traefik.internalIamHosts`
(plain hostnames, empty by default; no public route or network-policy changes),
`TRUSTED_PROXIES` from a nonempty peer list,
and the four signer path/issuer/kid variables when `internalContext.enabled`,
mounting the existing signing Secret read-only. Active managed settings reject
duplicates in runtimeConfig data and extraEnv. Optional gRPC routes use h2c with
higher priority only on API hosts; the identity host, ordinary HTTP and WebSocket
routes keep HTTP transport. Guide:
`k8s/stargate/README.md`, "Traefik in front".

In edge mode the chart sets the managed `DATA_PATH` from
`persistence.mountPath`; a custom path is mounted together with
`/app/.stargate` (bundled applications, email templates, signing keys) from the
same claim, while the gateway and policy seeds stay read-only.

An optional `traefik.rateLimit` middleware precedes every IngressRoute, including
admin and gRPC routes. Its in-memory IP buckets survive Redis outages independently
of Stargate. Allowances multiply with Traefik replicas and router instances; depth defaults to the
admin allow-list's IP strategy. The production overlay enables the backstop.

An optional Traefik IP allow-list protects the identity host's `/admin` and
`/stargate` prefixes. Its route takes priority over gRPC, and the public identity
routes exclude protected and ambiguous paths to prevent fallback around the
middleware. Administrators' browser IPs must be allowed; canonical `/auth` and
`/oauth` paths remain public. Direct Service access needs separate network controls.

An optional cluster-only pre-install/pre-upgrade migration Job uses dedicated
hook configuration and optional DDL credentials. It sets the chart-managed
`DB_MIGRATE_ON_START=false` explicitly in the server, bootstrap, and import
environment so they verify the schema even with external runtime resources.
Schema verification compares migration versions and checksums and reports a
modified applied migration when a checksum differs.
`extraVolumes` and `extraVolumeMounts` reach the server, migration Job and both
bootstrap/config-import hooks; built-in data/seed mounts are retained. The Helm
connection-test Pod uses a pinned BusyBox index (catalogued under
`k8s/stargate/VERSIONS.md` and `versions.json`), bounded resources and UID/GID 65534.
Migration pods use distinct labels and need their own egress
policy when the namespace denies traffic by default. Old replicas keep serving
during the migration, so schema changes must remain compatible with them.

`values-production.yaml` is a hardening overlay for `values-traefik.yaml`
(verified PostgreSQL/Redis TLS, super-admin MFA and step-up, admin allow-list,
migration Job, node spread, `maxUnavailable: 1`); `docs/production-checklist.md`
lists the remaining operator steps. Spread constraints without `labelSelector`
get the release's server pod selector. Chart tests: `k8s/stargate/tests/`.

Stargate exports OTLP and ships no backend-specific observability resources.
`docs/monitoring.md` defines the OpenTelemetry metric contract, recommended views
and an alert catalog. Operators implement dashboards and alert rules in their
infrastructure repository and configure the collector's destination.

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
  `Path=/oauth`), the callback `/oauth/{provider}` consumes the state atomically,
  signs in (`start_user_session`, amr `fed`), and redirects to the auth app's org
  picker then `return_to`, or to its login page with `error=social_*`.
  `return_to` must be a path or a URL on the issuer /
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
  `{i:<id>}:authorize` (10 min) and redirects to
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
  Otherwise the request is parked (`{i:<id>}:logout`, 10 min, id in the
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
  Each key is staged beside `keys/` and renamed into place, so nodes starting together
  on a shared fresh keyring load the first published key. `alg` is inferred from the public key when absent. Tokens without a known `kid` are
  rejected. `JWT_ALGORITHM` mismatch with the active key fails startup.
- Rotation: generate next key + restart (published), wait JWKS cache, set `JWT_KID`
  - restart, remove the old dir after `JWT_REFRESH_EXP`. HMAC (`HS*`) is single-key.
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
  `picture`, `locale`, `updated_at`
- `offline_access`: refresh tokens when the client allows `refresh_token`
- ID tokens carry no private `typ` / `sub_id` claims.
- ID tokens and OAuth access tokens carry `acr` and `amr`; introspection adds them with
  `auth_time`; codes and refresh families keep the grant's `Authentication`.

Provider state:

- SQL: `oauth_clients`, `oauth_consents`.
- Store: `{c:<code_hash>}:code` (pending/redeemed, TTL),
  `{f:<family_id>}:refresh` (CAS rotation), `{i:<id>}:authorize`,
  `{i:<id>}:logout`, `{a:<client_id>}:assertion:<jti_hash>` (assertion replay,
  until `exp`), and JWT `jti` revocations under `{j:<jti>}:revoked` until expiry
  (`store::keys::oauth`).

Client notes:

- Client secrets are returned only on create/rotate and stored as hashes.
- Logout metadata are columns (`post_logout_redirect_uris`, same rules as
  `redirect_uris`; `backchannel_logout_uri`, absolute HTTPS or loopback HTTP, no
  fragment; `backchannel_logout_session_required`, informational since `sid` is always sent).
- `initiate_login_uri` (column, same URL rules as `backchannel_logout_uri`) is where
  Stargate sends a user once an invitation, a self-registration, or a password reset
  that began in that app completes (OpenID Connect Core 1.0 §4; `iam/oidc/continuation.rs`).
  Completions return `continueUrl` (`iss`, `login_hint`, optional `target_link_uri`) and
  `clientName`; the URL is built on the server and the hosted apps never read a
  destination from the query.
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

OTP state keys follow the schema in `docs/redis-keys.md`: passwordless and
pending-login challenges under `{otp:<challenge_id>}`, the authenticated challenge
under `{u:<user_id>}:mfa:<challenge_id>`, the step-up marker under
`{s:<sid>}:mfa:<purpose>`, and request throttling under `{ip:<ip>}:otp-request`
and `{otp:<subject hash>}:requests:<ip>`.

Delivery: `smtp::Smtp` on `spawn_blocking`; passwordless delivery is backgrounded
against enumeration timing. `SMTP_TLS` selects `starttls` (default, required
STARTTLS), `implicit` (SMTPS), or `none` (local relays only; rejected at
startup with credentials). Only email OTP is wired; TOTP/HOTP and SMS exist as
primitives only.

Per-recipient email throttle (`src/iam/email_throttle.rs`): unauthenticated flows
that email an address from the request body (`POST /account/credentials`,
`POST /signup`, `POST /account/login/otp/email`) charge
`{em:<sha256(lowercased, trimmed address)>}:email:<flow>` before any
account lookup, so denials look the same for unknown addresses. Limits per hour:
5 for password reset and signup, 10 for passwordless OTP; flows have separate
buckets. Over-limit requests are not counted (the window does not slide under a
flood) and get `429 email.recipient_rate_limited` with `Retry-After: 3600`.
Admin invitations reuse `send_signup_request` without this throttle.

## Passwords

Argon2id with tunable params and optional pepper (hash upgrades on login), global
password policy from `PASSWORD_POLICY_*` with per-org override in
`organizations.attrs.password_policy`, history in `credential_history`.
Guide: `docs/password-policies-guide.md`. Process-wide `PASSWORD_HASH_CONCURRENCY`
(default available CPUs, positive integer) bounds all Argon2 work. Foreground
hash/verify/history admission waits at most 5s and returns `503 iam.overloaded`
(`Retry-After: 10`) without recording a failed login. Background rehash never
waits: it skips when busy and retries on a later login. The blocking closure owns
its permit even after client cancellation. Startup precomputes the dummy hash.
Metrics: `stargate.iam.password_hashing.active` and `.rejections`, with rejection
attribute `stargate.operation=verify|hash|history|rehash_skipped`.

Password reset and expired-password completion consume the pending request before
writing the new password and revoke all of the user's login sessions, forgetting
every address recognized by the login guard. Linked OAuth
refresh families are rejected and removed when used after the session ends.
Reset, signup email-verification and signup completion JWTs require separate
purpose types (`pwd_reset`, `signup_verification`, `signup`); untyped tokens are
rejected. Each credential token lasts one hour and carries its own `jti`.

Forgot password (`POST /account/credentials`): always `200 password.reset_requested`,
whether or not an account uses the address (only exact email matches get a link,
never nickname/phone). The JWT and SMTP delivery run on `spawn_blocking`, so
response time does not reveal the account either; delivery errors are only logged.
A pending request (`{em:<email hash>}:password-reset`, 1h) is reused and its link
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

Continuations: invitations accept `clientId` (the client must exist, be enabled, and
register `initiate_login_uri`, else `400 signup.invitation_client_invalid`) and an
optional `targetLinkUri` (absolute HTTPS, only with `clientId`). `POST /signup` and
`POST /account/credentials` accept `interactionId`, the parked authorization request
the app derived from its `return_to`; the pending signup or reset keeps the client, so
the emailed link works in another tab and after the interaction expires. A
self-service signup or reset follows the app of its newest link and renews the
pending request with it; an invitation keeps the app the administrator chose.
Missing or expired interactions mean no continuation, never an error.

SMTP sender and recipient addresses reject non-ASCII whitespace and control
characters. Signup reports these addresses as `400 request.invalid`.
A quoted local part in `SMTP_FROM` is preserved.

Emails: templates come from `apps/mail/emails/*.tsx`; `npm run export` renders
`<template>.<locale>.html` and `subjects.json` into `.stargate/transactional`
for every supported locale (`en`, `it`). Templates and subjects are localized;
the server validates that every template and subject exists for every locale at
startup. Delivery locale order is the account's `users.attrs.locale`, the
request's `Accept-Language` for new addresses, then `DEFAULT_LOCALE` (default
`en`). Action URLs are built by the server
(`src/web/links.rs`: `signup_url`, `change_password_url`, `login_url`, based on
`AUTH_PUBLIC_BASE_URL`, else the public base URL plus the auth app base path) and
rendered as `{{link}}`; `{{name}}`, `{{code}}`, and `{{minutes}}` are the other
supported placeholders.

## Database Schema (migrations/)

Tables: `organizations`, `users`, `super_admins`, `credentials`, `credential_history`,
`api_keys`, `admin_keys`, `service_accounts`, `oauth_clients`, `oauth_consents`,
`user_organizations`, `user_api_keys`, `service_account_api_keys`, `outbox_events`,
`config_documents` (append-only gateway configuration and ACE policy versions),
`quota_config` (last activated gateway document version),
`quota_definitions` (active fingerprints and the version from which each can
reserve), and `quota_usage`
(durable calendar-window quota balance; replicas reserve batches
with one conditional upsert; balances of deleted subjects and organizations are
pruned in batches every ten minutes).
The initial migration per backend (`migrations/{postgres,sqlite}/..._init_schema.sql`)
creates the complete initial schema. After a release, published migrations are
immutable: never edit or squash them. Every schema change must be a new forward
migration. Local databases created before the initial schema was consolidated
must be recreated, because sqlx rejects a modified applied migration checksum.

User identity (`crates/db/src/ent/user.rs`, enforced by the users repository on
every insert/update):

- `email` and `nickname` are stored canonical (trimmed, lowercased:
  `canonical_email` / `canonical_nickname`) and looked up the same way, so one
  mailbox or handle is one account whatever the casing. Both columns are
  `VARCHAR(254)` on both backends. An address is at most 254 octets
  (`MAX_EMAIL_LEN`, RFC 5321 §4.5.3.1.3); the nickname limit (`MAX_NICKNAME_LEN`)
  matches because its default is the email. Every write checks the lengths
  because SQLite ignores `VARCHAR` lengths.
- `nickname_is_valid`: a nickname is non-empty, at most 254 octets, and may
  contain `@` only when it equals the account's own email (the default
  nickname), so no account can take another person's address as its handle. A
  nickname that mirrored the email follows it when the email changes
  (`update_user`).
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

| Var                                                                                    | Default                                | Purpose                                                                                                                             |
| -------------------------------------------------------------------------------------- | -------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `CONFIG_PATH`                                                                          | `.stargate`                            | Gateway/policy seed directory (read-only in the chart)                                                                              |
| `CONFIG_FILENAME`                                                                      | `config.yaml`                          | Gateway seed filename, used only when its database document is absent                                                               |
| `DATA_PATH`                                                                            | `.stargate`                            | Writable edge directory for `sqlite.db` and `memory.bin`; keep it distinct from a read-only `CONFIG_PATH` (for example `CONFIG_PATH=/etc/stargate` and `DATA_PATH=/var/lib/stargate`) |
| `CONFIG_SYNC_INTERVAL_SECS`                                                            | 30                                     | Gateway/policy reconciliation interval in both profiles (integer >= 1); validated at startup                                        |
| `PORT`                                                                                 | 5050                                   | Port of the implicit listener; stops startup with `LISTENERS`                                                                        |
| `MAX_CONNECTIONS`                                                                      | 1024                                   | Concurrent connections of the implicit listener; stops startup with `LISTENERS`                                                     |
| `LISTENERS` / `LISTENER_<NAME>_*`                                                      | -                                      | Named listeners: `_ADDRESS` and `_PLANES` required, `_TLS` on except management, `_MAX_CONNECTIONS` 1024, `_TRUSTED_PROXIES` none; needs `JWT_ISSUER` (see "Listeners") |
| `STARGATE_RUNTIME_PROFILE`                                                             | -                                      | `edge` or `cluster` (validated against the build)                                                                                   |
| `JWT_ALGORITHM`                                                                        | active key / `RS256`                   | Must match the active key when set; picks the first generated key                                                                   |
| `JWT_SECRET`                                                                           | -                                      | HMAC key for HS* algorithms                                                                                                         |
| `JWT_ISSUER`                                                                           | `http://localhost:$PORT`               | JWT and OAuth/OIDC issuer                                                                                                           |
| `JWT_KID`                                                                              | `stargate-current`                     | Active signing key (`.stargate/jwks/keys/<kid>/`)                                                                                   |
| `JWT_ID_TOKEN_KID`                                                                     | -                                      | Optional RS256 key that signs ID tokens (EC or HMAC active key)                                                                     |
| `JWT_ACCESS_EXP` / `JWT_REFRESH_EXP`                                                   | 1h / 1d                                | Token TTLs                                                                                                                          |
| `OAUTH_BASE_URL`                                                                       | issuer                                 | Public base URL (`conf::public_base_url`): provider endpoints, email links, OpenAPI `servers`                                       |
| `IAM_HOSTS`                                                                            | all hosts                              | Comma-separated IAM hostnames, case-insensitive, without schemes, ports, paths, or wildcards; must include the public base URL host |
| `JWKS_CACHE_MAX_AGE_SECS`                                                              | 300                                    | JWKS cache max-age                                                                                                                  |
| `OAUTH_CLIENT_CA_FILE`                                                                 | -                                      | PEM roots trusted (with the system ones) for OAuth client endpoints (back-channel logout, `jwks_uri`); read and validated at startup |
| `INTERNAL_CONTEXT_*`                                                                   | see `.env.example`                     | Internal-context issuer, kid, key/JWKS paths, TTL, skew, signing workers/queue                                                      |
| `POSTGRES_ENDPOINT`                                                                    | localhost:5432                         | Cluster DB endpoint: host, host:port, [ipv6], or [ipv6]:port; invalid endpoints stop startup                                        |
| `POSTGRES_USERNAME` / `_PASSWORD` / `_DATABASE`                                        | root / root / stargate                 | PostgreSQL fields passed directly to sqlx; no URL encoding                                                                          |
| `POSTGRES_SSLMODE`                                                                     | prefer                                 | disable, prefer, require, verify-ca, or verify-full; other values stop startup                                                      |
| `POSTGRES_SSLROOTCERT`                                                                 | -                                      | Private root CA PEM path for PostgreSQL TLS                                                                                         |
| `POSTGRES_CREATE_DATABASE`                                                             | true                                   | true/false; false skips maintenance DB access and CREATE DATABASE                                                                   |
| `DB_POOL_MAX`                                                                          | 20                                     | Positive maximum connections in each SQL pool; invalid values stop startup                                                                                                |
| `DB_MIGRATE_ON_START`                                                                  | true                                   | true/false; false checks the schema without migrating; run `stargate db migrate` separately                                         |
| `REDIS_URL`                                                                            | redis://localhost:6379                 | Redis endpoint; `rediss://` verifies the server and hostname                                                                        |
| `REDIS_KEY_PREFIX` | empty | Literal prefix before the `stargate:v1:` schema root, including limiter Lua keys; braces are rejected; excludes audit stream names. Coordinate changes across replicas (`docs/redis-keys.md`). |
| `REDIS_POOL_SIZE` | 4 | Positive connections per Redis pool; invalid values stop startup |
| `REDIS_USERNAME` / `REDIS_PASSWORD`                                                    | -                                      | Optional ACL credentials, passed without URL encoding; cannot be combined with URL credentials                                      |
| `REDIS_SECURITY_TIMEOUT_MS` | 1000 | Positive total Redis time for request security state, including ingress wait |
| `REDIS_WAIT_REPLICAS` | 0 | Number of replicas to acknowledge security writes. Zero disables WAIT |
| `REDIS_WAIT_TIMEOUT_MS` | 250 | Positive milliseconds to wait for replica acknowledgments |
| `REDIS_WAIT_FAIL_ON_TIMEOUT` | true | Return `503` on replication shortfall. With false, warn and return success |
| `REDIS_CA_FILE`                                                                        | -                                      | Private CA PEM roots for `rediss://`; replace public roots                                                                          |
| `STORE_MAX_MEMORY_MB`                                                                  | 100                                    | In-memory store limit in MiB (`edge`); positive integer, invalid values stop startup                                                |
| `RUST_LOG`                                                                             | -                                      | Tracing/log filter directives                                                                                                       |
| `LOG_FORMAT`                                                                           | pretty (debug), json (release)         | Console format: json, compact, or pretty; invalid values stop logging initialization                                                |
| `LOG_CONSOLE_ENABLED`                                                                  | true                                   | Log to stdout; stays enabled if both sinks are disabled                                                                             |
| `LOG_FILE_ENABLED`                                                                     | false                                  | Enable optional JSON log files                                                                                                      |
| `LOG_DIR`                                                                              | .stargate/logs                         | Directory used only when file logging is enabled                                                                                    |
| `OTEL_RESOURCE_ATTRIBUTES` | SDK environment attributes | Comma-separated resource attributes; optional explicit `service.instance.id`, otherwise a per-process ULID. Chart-managed from pod identity. |
| `POD_NAME` / `POD_NAMESPACE` | downward API (chart) | Used by Kubernetes to expand the chart-managed telemetry resource attributes. |
| `OTEL_ENABLED`                                                                         | auto                                   | OTLP export; on when an endpoint is set                                                                                             |
| `OTEL_EXPORTER_OTLP_ENDPOINT` / `_PROTOCOL`                                            | - / grpc                               | OTLP target (per-signal overrides exist)                                                                                            |
| `AUDIT_RELAY_ENABLED`                                                                  | true                                   | Run the outbox relay (cluster)                                                                                                      |
| `AUDIT_RELAY_BATCH` / `AUDIT_RELAY_INTERVAL_MS`                                        | 256 / 1000                             | Relay batch size / idle poll                                                                                                        |
| `AUDIT_REDIS_STREAM`                                                                   | audit.raw                              | Redis stream for raw audit events                                                                                                   |
| `AUDIT_STREAM_MAXLEN`                                                                  | 100000                                 | Stream capacity target (protected trimming with groups; 0 = unbounded)                                                                                       |
| `AUDIT_STREAM_CONSUMER_GROUPS` | empty | Distinct comma-separated consumer groups; protect unacknowledged stream entries |
| `AUDIT_OUTBOX_RETENTION` | 30d | Cluster published-row retention (duration or `none`); never prunes pending rows |
| `TLS_ENABLED`, `TLS_CERT`, `TLS_KEY`                                                   | false                                  | TLS of the implicit listener (`TLS_ENABLED` stops startup with `LISTENERS`); the certificate is shared. With listener TLS off, an `https://` public base URL still marks the deployment HTTPS |
| `TRUSTED_PROXIES`                                                                      | -                                      | Implicit listener's proxy sources; refused with `LISTENERS` (use `LISTENER_<NAME>_TRUSTED_PROXIES`)                                  |
| `CORS_ORIGINS`                                                                         | -                                      | Allowed CORS origins (or `*` alone); invalid entries stop startup                                                                   |
| `TRUSTED_ORIGINS`                                                                      | -                                      | Trusted browser origins for sensitive IAM endpoints (no wildcard); invalid entries stop startup                                     |
| `GEOIP_DB_PATH`                                                                        | -                                      | MaxMind GeoLite2                                                                                                                    |
| `SERVER_DRAIN_DELAY_SECS`                                                              | 5                                      | Readiness-off delay before listeners close                                                                                          |
| `SERVER_SHUTDOWN_TIMEOUT_SECS`                                                         | 25                                     | Connection drain deadline                                                                                                           |
| `AUTH_APP_DIR` / `AUTH_APP_BASE_PATH`                                                  | `.stargate/apps/auth` / `/auth`        | Hosted auth app files and mount (empty or `/` = default)                                                                            |
| `AUTH_PUBLIC_BASE_URL`                                                                 | -                                      | Public auth app URL when hosted elsewhere                                                                                           |
| `CONSOLE_APP_DIR` / `CONSOLE_APP_BASE_PATH`                                            | `.stargate/apps/console` / `/stargate` | Console files and mount (empty or `/` = default)                                                                                    |
| `SIGNUP_MODE` / `SIGNUP_ALLOWED_DOMAINS`                                               | invite / -                             | Who may create accounts (`open`, `invite`, `closed`); domains for `open`                                                            |
| `PASSWORDLESS_LOGIN_ENABLED`                                                           | false                                  | Passwordless email-code login (needs `EMAIL_OTP_PEPPER`; not with `MFA_MODE=required`)                                              |
| `CONSOLE_ENABLED`                                                                      | true                                   | Serve the Console app (`false`: its path answers `404 feature.disabled`)                                                            |
| `CONSOLE_OAUTH_CLIENT_ID`                                                              | stargate_console                       | Reserved Console OAuth client                                                                                                       |
| `CONSOLE_OAUTH_REDIRECT_URI`, `CONSOLE_PUBLIC_URL`                                     | derived                                | Console callback / origin                                                                                                           |
| `EMAIL_OTP_PEPPER`, `EMAIL_OTP_LENGTH`, `EMAIL_OTP_TTL_SECS`, `EMAIL_OTP_MAX_ATTEMPTS` | -, 6, 300, 5                           | Email OTP                                                                                                                           |
| `MFA_MODE`                                                                             | optional                               | `off`, `optional`, `required`                                                                                                       |
| `MFA_METHODS` / `MFA_DEFAULT_METHOD` / `MFA_EMAIL_ENABLED`                             | email / email / true                   | MFA methods                                                                                                                         |
| `MFA_REQUIRED_FOR_SUPER_ADMIN`                                                         | false                                  | MFA for super-admins in `optional` mode                                                                                             |
| `MFA_ADMIN_STEP_UP_REQUIRED` / `MFA_STEP_UP_TTL_SECS`                                  | false / 300                            | Admin step-up                                                                                                                       |
| `PASSWORD_POLICY_*`                                                                    | see `.env.example`                     | Global password policy                                                                                                              |
| `ARGON2_MEMORY_KIB` / `_ITERATIONS` / `_PARALLELISM`                                   | 19456 / 2 / 1                          | Argon2id cost                                                                                                                       |
| `PASSWORD_HASH_CONCURRENCY` | available CPUs (at least 1) | Positive process-wide Argon2 concurrency; invalid values stop startup |
| `PASSWORD_PEPPER`                                                                      | -                                      | Optional Argon2 secret                                                                                                              |
| `SMTP_*` (incl. `SMTP_TLS`), `GOOGLE_OAUTH_*`, `GITHUB_OAUTH_*`           | -                                      | Email delivery (`SMTP_TLS=starttls` default), social login                                                                              |
| `DEFAULT_LOCALE`                                                                       | `en`                                   | Outgoing email and invitation default language; `en` or `it`                                                                         |

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

Dependency versions live in `[workspace.dependencies]` in the root `Cargo.toml`.
Member manifests inherit them with `{ workspace = true }` and only add `features`,
`optional`, or `default-features`; bump a version in that one place. Backends must
stay feature-gated per crate (`memory`/`redis`, `sqlite`/`postgres`), so a
`redis` or `sqlx` dependency in a leaf crate is `optional` and enabled by that
crate's backend feature.

SQLite temporarily patches the released `sqlx-sqlite 0.9.0` driver through
`vendor/sqlx-sqlite` to permit `libsqlite3-sys >=0.39.1` (bundled SQLite 3.54.1,
RUSTSEC-2026-0459). Rust sources are unchanged. Details, integrity pins and the
removal condition are in `vendor/README.md`, `VERSIONS.md` and `versions.json`.
Keep the vendor directory available in Docker planner/cook/build stages.

### CI Workflows

GitHub Actions checks pull requests; a merge to `main` starts no CI run, so the
checks on a pull request updated with `main` cover the tree the squash merge
creates.

| Workflow | Runs |
|:--|:--|
| `ci.yml` | Every pull request, weekly on `main` (Monday at 03:43 UTC), and as the reusable test job of `release.yml` (tags and manual release runs). |
| `oidc-conformance.yml` | Every pull request and manual dispatch. The suite runs only when the changes match `.github/paths/oidc-conformance.txt`; otherwise the required `OIDC conformance` summary job reports success. |
| `redis-failures.yml` | Every pull request and manual dispatch. The suites run only when the changes match `.github/paths/redis-failures.txt`; otherwise the required `Redis suites` summary job reports success. |
| `dependency-audit.yml` | Daily at 04:17 UTC and manual dispatch: `cargo audit` and `.github/scripts/check-npm-audit.py` for `apps/auth`, `apps/console` and `apps/mail`. |
| `release.yml` | Tags `v*.*.*` and manual dispatch. |

Rust and npm audits run on every pull request, independently of application
lint and build checks. Their required `Dependency audits` summary enforces the
results only when paths match `.github/paths/dependency-audit.txt`: Cargo and npm
manifests or lockfiles, `.cargo/audit.toml`, and
`.github/npm-audit-exceptions.toml`. On other pull requests audit failures remain
visible and informational; application lint and build checks stay required.
Weekly and release runs always enforce the audits.

## CLI

```bash
# First super-admin + reserved Console OAuth client (idempotent)
cargo run --features edge -- admin bootstrap \
  --email admin@example.com --generate-password \
  --name "Admin User" --nickname admin \
  --oauth-redirect-uri https://identity.example.com/stargate/auth/callback

# List, back up or import exact database document content (both profiles)
cargo run --features edge -- config versions --kind gateway --limit 20
cargo run --features edge -- config export --kind gateway --version 41 --output gateway-v41.yaml
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
`config versions` lists metadata newest first and marks the database's active version;
`config export --version N` retrieves that exact historical document. Export/import
restores earlier content by appending a new audited version; capacity changes still
require restart. Database history does not replace external backups.
Imports preserve the submitted text, retry a revision conflict once, and write an
atomic `cli_import` system audit event for changed content. Runtime replicas do full
preparation before activation. The Helm ConfigMap is a seed by default; cluster-only
`gatewayConfig.importOnUpgrade` imports both documents on install/upgrade and replaces
Console edits. Edge imports use `kubectl exec` in the existing pod. Docs: `docs/cli.md`.

## API Overview

- `GET /livez` (process liveness), `GET /readyz` (initialized, not draining, SQL up).
  Redis does not gate readiness. Traffic limits fail open by default on store
  errors/timeouts; public routes, static pages, JWKS and SQL-verified admin keys
  remain usable. `/admin/health` returns `200` with healthy SQL even when Redis is
  unhealthy and exposes `limits.storeDegraded`. Login guard, OTP/email throttles,
  sessions/JWT revocation and OAuth state remain fail-closed. See `docs/health-probes.md`.
- `GET /docs` (OpenAPI JSON for the receiving listener’s planes; served with identity/admin), `GET /docs/errors` (error catalog), `GET /i18n/{locale}` (`en`, `it`)
- `/.well-known/*`: OAuth/OIDC metadata, JWKS, `stargate-context-jwks.json`
- `/account/*`: login, logout, profile, refresh, credentials, organizations, consents, OTP/MFA,
  `features` (public: available sign-in options)
- `GET /account/profile` reads the signed-in user's profile; `PUT /account/profile`
  saves their preferred language (`locale`, `en` or `it`) without writing or emitting
  audit events when unchanged. Both accept native session tokens or access tokens
  from an enabled first-party OAuth client, with an active session belonging to
  the same user. Third-party OAuth clients must use scoped `/oauth/userinfo`.
  Locale saves use a dedicated database method that checks the actor and merges
  only `attrs.locale` into the locked current row. Its `user.updated` audit event
  has source `account` and snapshots containing only the locale.
- `/oauth/*`: provider endpoints; `/oauth/state`, `/oauth/github`, `/oauth/google`: social login
- `/signup/*`: registration + email verification
- `/admin/*`: users (+ invitations), orgs, API/admin keys, OAuth clients, service accounts,
  config, access-control rules, sessions, outbox events, `me`, `overview`, `health`.
  Admin credentials come only from `Authorization: Bearer` or `x-api-key`, including
  admin privileges on `/oauth/introspect` and `/oauth/revoke`. Session cookies and
  query parameters grant no admin privileges on any of these routes. Every admin
  endpoint documents `503 auth.state_unavailable` with `Retry-After: 10`.
  Super-admin users bypass capability checks; admin keys need exact `<resource>:<action>`
  permissions (`src/api/admin/authorization.rs`).
  Capabilities never permit mutations of super-admin accounts: profile/attrs,
  memberships, sessions and owned API keys require a super-admin principal.
  Updating/deleting an organization containing a super-admin is protected too,
  because policies, memberships and sessions would affect that account.
  First-party OAuth clients (trusted for admin access) can only be created or
  mutated by super-admins, including changes to `attrs.first_party`.
  Super-admins keep self/other-super-admin edits; deleting a super-admin remains
  forbidden to everyone. User API keys authenticate gateway API-key subjects,
  never an admin user, even when owned by a super-admin. Guide: `docs/admin-keys.md`.
- `/auth/*` (hosted auth app), `/stargate/*` (console), everything else: gateway fallback.

`IAM_HOSTS` optionally restricts every IAM route above (including docs and hosted
apps) to exact hostnames. Unset or empty preserves IAM routes on every host.
Entries are validated once at startup, compared in lowercase without the request
port, and must include the host of `conf::public_base_url()` (`OAUTH_BASE_URL`, else
the issuer). Schemes, ports, paths, and wildcards in entries stop startup.

`api::listener_router(planes, lifecycle, hosts)` composes the router a listener
serves from the planes it declares (`api::plane::PlaneSet`): management (`/livez`,
`/readyz`), identity (well-known metadata, OIDC, accounts, sign-up, social
sign-in, the hosted auth app, docs, i18n, the internal-context JWKS), admin
(`/admin/*` and the Console), and the gateway. `api::server_router` is the same
composition with every plane, for tests. Probes are mounted on every host,
including pod IPs and hostless requests. `dispatch::HostDispatch` sends allowed
hosts through the IAM planes and their shared ingress limit; the gateway is their
fallback only when the listener serves it. Other hosts and hostless requests go
directly to the gateway when `IAM_HOSTS` is configured, where the gateway ingress
limit still applies. Path normalization, CORS, and compression belong to the
planes Stargate answers itself: a trailing slash is trimmed before those routes
match and the gateway plane restores the client's URI, so an upstream receives
the path the client sent and owns its own preflights and content encoding.
Listeners without the admin plane reserve `/admin` and the configured Console
base path (including their descendants) on IAM hosts with `404`, even with a
catch-all gateway router. Other hosts keep gateway routing. `/docs` is served on
identity and admin listeners and includes only the planes that listener serves.
IAM APIs never authenticate session/access tokens from query parameters. Signup
verification (`GET /signup`) takes its purpose token in `Authorization: Bearer`;
the auth app extracts it from the emailed page URL before making that API call.
OAuth/OIDC protocol parameters such as `code`, `state`, and `id_token_hint` retain
their specified transport; UserInfo POST still accepts form `access_token`.

The origin guard is active by default on sensitive account and signup routes, social state,
OIDC login/consent, interaction reads and logout confirmation. It trusts the public
base URL's origin plus explicit `TRUSTED_ORIGINS`; neither request Host nor CORS
adds trust. An untrusted/opaque/duplicate Origin is rejected; without Origin,
`Sec-Fetch-Site: same-site|cross-site` is rejected too. Requests without either
header remain usable by non-browser clients; OPTIONS passes to CORS. OIDC
`/oauth/authorize` and `/oauth/logout` retain cross-origin browser entry for their
specified RP flows. The server warns at startup when `IAM_HOSTS` is empty and a
gateway router can match without a positive host constraint.
`main.rs` adds security headers and tracing to every listener's router.

IAM limits (`src/api/middleware/rate_limit.rs`): every IAM route charges the client IP's
ingress allowance, the same `{ip:<ip>}:ingress` bucket as the gateway, before
credentials are read (`429 iam.ingress_rate_limit_exceeded`). Authenticated
`/admin/*` principals then charge their own `default` bucket, keyed
`{admin:user:<id>}:requests` / `{admin:key:<id>}:requests` in
`extract_admin_authorization`
(`429 iam.rate_limit_exceeded`, scope `subject`), so one admin key cannot throttle
the console or other keys behind the same IP. `ingress.on_store_error` governs both IAM checks: store errors/timeouts pass
without allowance headers by default; `deny` returns `503 iam.limiter_unavailable`
with `Retry-After: 10`. Invalid limit configuration remains `500`.
Password verification reserves an attempt atomically in each of three buckets
(`iam/auth/login_guard.rs`): per IP, per account and IP, and per account across IPs.
A valid password refunds its own reservations. The account-wide bucket uses
bounded progressive backoff. A completed password login (JSON, OAuth form or MFA
completion) recognizes the server-resolved account/IP pair for 30 days. This
skips only the account-wide pause. The per-IP and account/IP limits always apply.
A password reset or expired-password completion, and any revocation of every
session, deletes the account's recognized addresses, so none stays exempt from
the account-wide pause.
See "Password login guard" in `docs/config-v1.md` for defaults and retry behavior.
OTP flows have their own throttles.

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
closes and active work is cancelled (WebSockets send Close `1001`, `gateway shutting down`,
to both peers in parallel) -> every listener stops accepting and its connections drain
(`SERVER_SHUTDOWN_TIMEOUT_SECS`) -> tracked mirror/WebSocket tasks drain -> stop configuration synchronization -> stop
audit relay -> save in-memory store -> flush telemetry.

WebSocket Close exchanges have a fixed 5s limit, shortened by the remaining server
shutdown budget, and retain admission until they finish. Idle expiry sends `1001`
with `idle timeout` (`request timeout` for an explicit router deadline); backend
loss sends `1011` (`upstream connection lost`) to the client, not `1014`, which
tungstenite rejects; client loss sends `1001` to the backend. Received Close
frames are forwarded unchanged. Closing runs outside the cancelled execution
guard; unresponsive peers log at debug level.

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
  Signup and password-reset completions show "Continue to <app>" from the server's
  `continueUrl`; the app sends `interactionId` (read from the `return_to` query,
  `lib/auth-flow.ts` `getInteractionId`) when the flow began in a client, and never
  reads a destination from the query.
- `mail`: transactional email templates (`react-email`), rendered at build time
  for every supported locale with localized subjects. A build-time template
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

- Barrel `components/index.ts` (`Shell`, `Page`, `Panel`, `Table`, `EntityDrawer`,
  `CodeBox`, `JsonAttributesForm`, `showNotification`, `useConfirmModal`, ...).
- Console pages are a `Page` (title, description, badges, actions) holding `Panel`
  surfaces (optional title, toolbar, actions); tables and editors go inside a panel.
  The look comes from `app/theme.ts` (Mantine theme) and the `--app-*` variables in
  `app/globals.css`; the sidebar groups and their labels are declared in
  `app/routes/layout.tsx`.
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
- `stargate.limits.store_errors` counts traffic limit failures by bounded `stargate.point`
  and `stargate.outcome` attributes. Each point logs warn on degradation (at most once per 60s)
  and a paired info on successful recovery; the window persists across reloads.
  Backend causes are logged at debug. State uses named fields and exhaustive
  point selection rather than enum-position array indexing.
- OTLP resources identify each replica with `service.instance.id`, preserving
  explicit resource attributes and defaulting to a stable ULID per process.
- Application-owned metric attributes use the `stargate.` namespace (`point`,
  `outcome`, `router`, `service`, `kind`, `phase`, `path`). RPC status uses stable
  `rpc.status_code` with canonical gRPC status names. Resource attributes use
  `stargate.profile`, `stargate.database_backend` and `stargate.state_backend`.
- Telemetry follows OpenTelemetry HTTP semantic conventions: server operations
  use matched routes, no query strings are recorded, and unknown methods become
  `_OTHER` in attributes. `http.server.request.duration` and custom duration
  histograms use seconds with explicit buckets. `docs/monitoring.md` defines
  metrics, attributes, aggregation and a backend-neutral alert catalog.
- SQL pool gauges sample the initialized pool; upstream gauges read the current
  snapshot and never advance circuit state. Metric attributes are bounded;
  URLs and request identities stay out of metric dimensions.
- All Redis and in-memory state keys are built in `store::keys` as
  `stargate:v1:{<entity>}:<name>`; scripts declare their keys and batch
  operations split by tag. `docs/redis-keys.md` is the operator and developer
  reference; the Redis CI job runs the single-instance and 3+3 cluster suites
  plus the per-path round-trip caps.
- `stargate.store.redis.commands` and `stargate.store.redis.commands_per_request`
  attribute gateway store work to the pipeline path that issued it
  (`stargate.path=anonymous`, `api_key`, `session`, `oauth`, `rate_limit`,
  `quota`). Counting lives in `crates/store` (`observe`, one task-local scope per
  stage): each request sent to Redis counts once (a command, a script call or a
  whole pipeline), not the commands a Lua script issues internally.
- Gateway performance workloads are opt-in (`src/gate/performance/`,
  `src/gate/tests/paths.rs`, `benches/scripts/`).

Cluster workload measurements and connection/memory calculations are in
`docs/sizing.md`; the external driver is `benches/scripts/benchmark_cluster.mjs`.
