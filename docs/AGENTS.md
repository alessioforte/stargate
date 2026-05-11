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
│   ├── act/           # Action handlers (login guard, oauth state, signup, email verify, change password) + shutdown signal
│   ├── api/           # Axum routers + handlers + gateway fallback service
│   ├── aud/           # Audit service - buffered async event bus
│   ├── cli/           # Admin CLI (bootstrap super-admin)
│   ├── db/            # DB pool init (feature-gated postgres/sqlite)
│   ├── err/           # HTTP error types -> JSON responses (axum IntoResponse)
│   ├── etc/           # Config, CORS, GeoIP, JWT, TLS, guards, middleware, reqctx, proxy utils
│   └── fun/           # Business logic (token gen, name format, super-admin)
├── crates/
│   ├── ace/           # Access Control Engine (ABAC policy evaluation)
│   ├── db/            # DB repo traits + sqlx implementations
│   ├── gate/          # Gateway config schema, compiler, runtime graph
│   ├── jwt/           # JWT encode/decode
│   ├── lb/            # Load balancing, health checks, circuit breaker core
│   ├── lim/           # Rate limiting + quota
│   ├── idp/           # External identity provider clients (Google, GitHub)
│   ├── oidc/          # Pure OAuth/OIDC provider helpers (PKCE, codes, refresh, claims, metadata)
│   ├── pw/            # Password hashing (argon2)
│   ├── smtp/          # Email delivery (lettre)
│   ├── store/         # State store (DashMap / Redis)
│   └── tools/         # Shared utilities
├── ddl/               # SQL schemas: postgres.sql, sqlite.sql
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
7. Buffer replayable requests when mirror traffic or response-status failover requires it.
8. Execute HTTP, WebSocket, or direct response and apply gateway/response headers.

Gateway modules:
- `routing.rs`: router and matcher evaluation.
- `middlewares.rs`: path and header transform application.
- `policies.rs`: auth, ACE, rate limit, quota policy execution.
- `planner.rs`: service graph expansion into an execution plan.
- `executor.rs`: upstream/direct-response execution, mirror dispatch, failover execution.
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

Full schema notes and examples: `docs/config-v2alpha1.md`.

### Traffic Management

Implemented:
- Weighted traffic splitting between services.
- Shadow / mirror traffic with discarded mirror responses.
- Failover on transport error.
- Failover on selected response status codes.
- Direct response routes.
- Fallback routes via low-priority catch-all routers.
- Request replay buffering for mirror/status failover.
- Replay body cap via `GATEWAY_REPLAY_BODY_LIMIT` (default `2MiB`).

Partially implemented:
- Circuit breaker core exists in `crates/lb`, and load-balancing strategies skip unavailable upstreams. Live gateway proxy failures/statuses are not yet fully wired into breaker state, and breaker thresholds/cooldowns are not config-exposed.
- HTTP/2 client support is enabled in the shared hyper client, but per-upstream protocol policy is not fully enforced.
- Upstream `transport.connect_timeout` exists. Read/idle/request timeout config is not complete.

Missing:
- Retry with backoff.
- Request hedging.
- gRPC proxying and gRPC-Web transcoding.
- GraphQL-aware routing and query-complexity limits.
- SSE-specific proxy handling as a first-class gateway feature.

### Audit Service (`src/aud/mod.rs`)

Singleton via `once_cell`. Async channel -> worker task:
- Buffers events in memory (`AUDIT_BUFFER_SIZE`, default 1024)
- Flushes every 5s or on threshold
- Retry with exponential backoff
- Graceful shutdown drains buffer before exit

### Singletons / Runtime State

- Database pool (`DB`)
- Audit service
- JWT config
- GeoIP database
- Gateway runtime config and compiled HTTP graph
- Gateway load balancers
- ACE policy engine
- Limiter/quota service

### Config Hot-Reload

`notify` watches `config.yaml` and policy files. Content hash prevents false positives.

Hot reload behavior:
- Parse and compile the new v2 config first.
- If valid, atomically swap the active gateway graph/balancers/policies.
- If invalid, log the error and keep the previous in-memory config active.

Admin config endpoints validate v2 config before saving.

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

## Database Schema (ddl/)

Tables: `organizations`, `users`, `super_admins`, `credentials`, `api_keys`, `admin_keys`, `audits`, `service_accounts`, `oauth_clients`, `oauth_consents`, `user_organizations`, `user_api_keys`, `service_account_api_keys`

IDs: ULID (TEXT). Audit has actor_type enum, action enum, JSON metadata.

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
| `POSTGRES_ENDPOINT` | - | Postgres host (cluster) |
| `POSTGRES_USERNAME` | - | Postgres user |
| `POSTGRES_PASSWORD` | - | Postgres password |
| `POSTGRES_DATABASE` | - | Postgres DB name |
| `REDIS_URL` | - | Redis (cluster) |
| `RUST_LOG` | - | Log filter |
| `LOG_DIR` | - | Log file dir |
| `AUDIT_BUFFER_SIZE` | 1024 | Audit event buffer |
| `TLS_ENABLED` | false | HTTPS |
| `TRUSTED_PROXIES` | - | IP/CIDR for client IP |
| `CORS_ORIGINS` | - | Allowed origins |
| `TRUSTED_ORIGINS` | - | Trusted browser origins for sensitive IAM endpoints |
| `GEOIP_DB_PATH` | - | MaxMind GeoLite2 |
| `GATEWAY_REPLAY_BODY_LIMIT` | 2MiB | Max buffered body for mirror/status-failover replay |
| `SERVER_SHUTDOWN_TIMEOUT_SECS` | 25 | Request drain timeout during shutdown |

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

## CLI: Bootstrap Super-Admin

```bash
cargo run -- admin bootstrap \
  --email admin@example.com \
  --generate-password \
  --name "Admin User" \
  --nickname admin
```

Flags: `--password <val>`, `--password-stdin`, `--generate-password`

## API Overview

- `GET /health` - health check
- `GET /docs` - Swagger UI
- `/.well-known/*` - OAuth/OIDC metadata and JWKS
- `/account/*` - login, logout, profile, account refresh tokens, credentials
- `/oauth/authorize`, `/oauth/token`, `/oauth/userinfo`, `/oauth/introspect`, `/oauth/revoke` - OAuth/OIDC provider endpoints
- `/oauth/state`, `/oauth/github`, `/oauth/google` - Google/GitHub consumer login endpoints
- `/signup/*` - registration + email verification
- `/admin/*` - users, orgs, API keys, OAuth clients, service accounts, config (super-admin)

## Error Response Format

Errors are JSON responses with request ID in the `x-request-id` response header.

Typical body shape:

```json
{
  "message": "...",
  "code": "not_found",
  "type": "invalid_request",
  "link": "https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/404"
}
```

HTTP codes currently used include: 400, 401, 403, 404, 409, 413, 429, 500, 502, 503.

## Rate Limit Response Headers

`x-ratelimit-limit`, `x-ratelimit-remaining`, `x-quota-limit`, `x-quota-remaining`, `retry-after`

## Graceful Shutdown

SIGTERM/SIGINT -> drain requests (25s default timeout) -> flush audit buffer -> close DB -> save in-memory state.

## Current Notes

- Request ID: ULID per request, in response header `x-request-id`, in audit logs.
- Thread-local LRU caches for ACE decisions are version-aware and invalidated on policy change.
- WebSocket proxying is supported in the gateway via `hyper-tungstenite`.
- HTTP proxying strips hop-by-hop headers and supports preserve-host behavior.
- GeoIP is available via MaxMind GeoLite2-City, but geo-aware routing is not yet first-class in v2 matchers.
- OpenAPI docs use `utoipa` with `utoipa-axum` + `utoipa-swagger-ui`.
- Middleware chain in `main.rs`: rate limit -> security headers -> trace -> CORS -> compression -> normalize-path.
- TLS server uses a custom hyper-util connection loop with per-connection graceful shutdown and drain deadline.
- OAuth/OIDC browser consent UI is not implemented; third-party clients without stored consent receive `consent_required`.
- OAuth/OIDC non-redirect errors still use Stargate's generic error envelope rather than full RFC-shaped error bodies.
