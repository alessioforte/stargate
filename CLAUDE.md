# Stargate — Project Context

## What It Is

IAM server + API gateway in Rust. Single binary, two deployment profiles:
- **Edge**: SQLite + in-memory store (single node)
- **Cluster**: PostgreSQL + Redis (horizontally scalable)

## Workspace Structure

```
stargate/
├── src/               # Main binary
│   ├── main.rs        # Entry: init logging, DB, services, Actix server
│   ├── act/           # Server lifecycle, graceful shutdown, signal handling
│   ├── api/           # REST endpoints (account, oauth, signup, admin)
│   ├── aud/           # Audit service — buffered async event bus
│   ├── cli/           # Admin CLI (bootstrap super-admin)
│   ├── db/            # DB pool init (feature-gated postgres/sqlite)
│   ├── err/           # HTTP error types → JSON responses
│   ├── etc/           # Config, CORS, GeoIP, JWT, TLS, guards, proxy utils
│   ├── fun/           # Business logic (token gen, name format, super-admin)
│   └── gtw/           # Gateway handler — 6-stage request pipeline
├── crates/
│   ├── ace/           # Access Control Engine (ABAC policy evaluation)
│   ├── db/            # DB repo traits + sqlx implementations
│   ├── gate/          # Gateway routing config types
│   ├── jwt/           # JWT encode/decode
│   ├── lb/            # Load balancing strategies
│   ├── lim/           # Rate limiting + quota
│   ├── oauth/         # OAuth provider clients (Google, GitHub, LinkedIn, FB)
│   ├── pw/            # Password hashing (argon2)
│   ├── smtp/          # Email delivery (lettre)
│   ├── store/         # State store (DashMap / Redis)
│   └── tools/         # Shared utilities
├── ddl/               # SQL schemas: postgres.sql, sqlite.sql
├── k8s/               # Kubernetes manifests
└── benches/           # Criterion benchmarks
```

## Key Architecture

### Stack
- **Runtime**: Tokio (multi-threaded)
- **HTTP server**: Actix-web 4
- **DB**: sqlx (feature-gated `postgres` or `sqlite`)
- **Auth**: JWT (jsonwebtoken), Argon2 passwords, RSA/HMAC signing
- **Serialization**: serde_json, serde_yaml, MessagePack
- **Observability**: tracing + tracing-subscriber + tracing-appender

### Gateway Pipeline (`src/gtw/mod.rs`)
6 stages, early bailout:
1. Service lookup (in-memory)
2. Route matching (in-memory)
3. Auth check (JWT or API key)
4. Access control — ACE policy eval, thread-local LRU cache (1024 entries)
5. Rate limit + quota (store round-trips)
6. Load balance + proxy (HTTP or WebSocket)

### Audit Service (`src/aud/mod.rs`)
Singleton via `once_cell`. Async channel → worker task:
- Buffers events in memory (`AUDIT_BUFFER_SIZE`, default 1024)
- Flushes every 5s or on threshold
- Retry with exponential backoff
- Graceful shutdown drains buffer before exit

### Singletons (once_cell)
- Database pool (`DB`)
- Audit service
- JWT config
- GeoIP database
- Gateway config (hot-reloaded via file watcher)

### Config Hot-Reload
`notify` crate watches `config.yaml` and policies files. Content hash prevents false positives. Reloads gateway config without restart.

### Feature Flags
```toml
edge    = ["sqlite", "memory"]
cluster = ["postgres", "redis"]
```
Compile-time backend selection — no runtime branching.

## Database Schema (ddl/)

Tables: `organizations`, `users`, `super_admins`, `credentials`, `api_keys`, `admin_keys`, `audits`, `service_accounts`, `user_organizations`, `user_api_keys`, `service_account_api_keys`

IDs: ULID (TEXT). Audit has actor_type enum, action enum, JSON metadata.

## Key Env Vars

| Var | Default | Purpose |
|-----|---------|---------|
| `PORT` | 5050 | HTTP listen port |
| `STARGATE_RUNTIME_PROFILE` | — | `edge` or `cluster` |
| `JWT_SECRET` | — | HMAC signing key |
| `JWT_ACCESS_EXP` | 1h | Access token TTL |
| `JWT_REFRESH_EXP` | 1d | Refresh token TTL |
| `POSTGRES_ENDPOINT` | — | Postgres host (cluster) |
| `POSTGRES_USERNAME` | — | Postgres user |
| `POSTGRES_PASSWORD` | — | Postgres password |
| `POSTGRES_DATABASE` | — | Postgres DB name |
| `REDIS_URL` | — | Redis (cluster) |
| `RUST_LOG` | — | Log filter |
| `LOG_DIR` | — | Log file dir |
| `AUDIT_BUFFER_SIZE` | 1024 | Audit event buffer |
| `TLS_ENABLED` | false | HTTPS |
| `TRUSTED_PROXIES` | — | IP/CIDR for client IP |
| `CORS_ORIGINS` | — | Allowed origins |
| `GEOIP_DB_PATH` | — | MaxMind GeoLite2 |

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

- `GET /health` — health check
- `GET /docs` — Swagger UI
- `/account/*` — login, logout, profile, refresh tokens, credentials
- `/oauth/*` — Google, GitHub, LinkedIn, Facebook flows
- `/signup/*` — registration + email verification
- `/admin/*` — users, orgs, API keys, service accounts, config (super-admin)

## Error Response Format

```json
{ "code": "ERROR_CODE", "message": "...", "request_id": "ulid" }
```

HTTP codes: 400, 401, 403, 404, 409, 429, 500, 502, 503

## Rate Limit Response Headers

`x-ratelimit-limit`, `x-ratelimit-remaining`, `x-quota-limit`, `x-quota-remaining`, `retry-after`

## Graceful Shutdown

SIGTERM/SIGINT → drain requests (25s timeout) → flush audit buffer → close DB → save in-memory state.

## Notes

- Request ID: ULID per request, in response header `x-request-id`, in audit logs
- Thread-local LRU caches for ACE decisions — version-aware invalidation on policy change
- WebSocket proxying supported in gateway (`actix-ws-proxy`)
- GeoIP via MaxMind GeoLite2-City
- OpenAPI docs via `utoipa`
