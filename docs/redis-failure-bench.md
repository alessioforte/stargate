# Redis failure bench

`benches/scripts/redis_failures.py` runs a disposable cluster-profile Stargate
with local PostgreSQL and Redis containers. It checks HTTP behavior, shared
traffic budgets and recovery. The partition holds the Redis connection open
while withholding all replies. The public gateway route has ingress, rate
and quota checks. The protected route accepts a native session, a gateway API
key or a user-delegated OAuth access token.

## Scenarios

| Scenario | Behavior asserted |
| --- | --- |
| `restart` | With AOF enabled, the existing session works once Redis returns. Failed-open requests get `200` without allowance headers. Failed-closed requests get `503` with `Retry-After`. Readiness stays healthy and Redis diagnostics report the outage. |
| `empty` | With persistence disabled, existing access and refresh tokens get `401` once Redis recovers. A fresh sign-in works. |
| `partition` | Gateway and IAM traffic follow their failure policies within one shared 250ms budget. Stateful auth requests and account flows (OTP/MFA, password reset, signup) return `503` with `Retry-After` using the one-second Redis budget; password-reset, signup and passwordless requests work again after recovery. A request already dispatched to its upstream completes, readiness stays healthy and the first recovered request charges its quota once. |
| `memory-full` | Redis rejects writes under `noeviction`. Session reads and PING work, login fails closed, and the limit store is reported degraded. Writes recover when memory is freed. |
| `absent-at-startup` | Stargate starts and serves readiness while Redis is down. Admin diagnostics show Redis unhealthy and session access fails closed. The existing process recovers when Redis starts. |
| `incompatible` | An older Redis without `HEXPIRE` is rejected with a capability diagnostic while Stargate serves readiness. |
| `replicated-logout` | With replica confirmation enabled, missing acknowledgments produce either an error or a warning according to settings. A successful logout stays revoked after promotion, including for the old refresh token. A subsequent logout everywhere revokes all three sessions on the promoted primary and reports the missing replica acknowledgment with `503`. |

The report records recovery per endpoint when the workload can probe it
continuously. The first successful traffic check must finish within one second
of Redis accepting writes, and every sampled endpoint must recover within five
seconds. The local relay resumes traffic at a known time for the partition test.
Outage latency checks allow 40ms for HTTP processing and scheduler jitter. The
Rust test checks the exact traffic budget against Tokio's clock.

## Run locally

```bash
cargo build --features cluster
python3 benches/scripts/redis_failures.py \
  --redis-image redis:7.4-alpine \
  --postgres-image postgres:17-alpine
```

Requirements: Docker, a `target/debug/stargate` built with the `cluster`
profile, and Redis 7.4+ (or Valkey 9.0+) with hash field expiry.
`--scenarios` selects a subset, `--unsupported-redis-image` changes the fixture
used for the capability rejection test, and `--output` writes the JSON report. Containers are disposable, bind to localhost
only and are removed on exit. The script exits non-zero at the first scenario
whose behavior does not match.

The project's other Redis suites are `benches/scripts/test_redis_prefix.py`,
which verifies the key schema prefix, the production ACL and the
session/limiter/relay smoke test on one instance, and
`benches/scripts/test_redis_cluster.py`, which runs the store and limiter tests
on a disposable 3+3 cluster. Run both with the same image; the key schema is in
`docs/redis-keys.md`.

## CI

`.github/workflows/redis-failures.yml` runs on pull requests that touch
`crates/store`, `crates/lim`, `src/data/store`, the session, authentication and
account-flow code (`src/iam/auth`, `src/iam/sess`, `src/iam/otp`, `src/iam/pwd`,
`src/iam/user`, `src/iam/email_throttle.rs`, `src/api/account`, `src/api/signup`),
the gateway limits and policies, startup and readiness (`src/main.rs`,
`src/srv/health.rs`), the admin health report (`src/api/admin/health.rs`), the
bench scripts and their fixture or this guide, and on manual dispatch:

```bash
gh workflow run redis-failures.yml --ref main
```

The job builds the cluster profile, runs the prefix/ACL smoke test and the 3+3
cluster store/limiter suites, then this bench with `redis:7.4-alpine` and
`postgres:17-alpine`. It ends with a short
`benches/scripts/benchmark_request_paths.py` run that fails when a gateway path
exceeds its Redis round-trip cap (`docs/gateway-performance.md`).

## Report

`disturbance_seconds` measures the time the induced failure was active.
`recovery_by_endpoint_ms` measures the first successful response per endpoint
after Redis accepts writes. `unexpected` is zero in a passing run. `errors`
counts failed responses and transport failures during the fault, including
expected `503` responses. The assertions include requests started far enough
inside the fault to complete within their budget.

## Recorded results

Recorded on 2026-11-23 on an Apple M1 Pro with macOS 27.2.1 and Docker 29.9.3.
The run used the Redis and PostgreSQL images in [Run locally](#run-locally)
and wrote the report with `--output`.

| Scenario | Requests | Unexpected | Errors | Disturbance (s) | Recovery (s) | Final state |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| `restart` | 210 | 0 | 44 | 1.77 | 0.17 | healthy |
| `empty` | 210 | 0 | 42 | 1.74 | 0.22 | signed-out then healthy |
| `partition` | 437 | 0 | 120 | 9.60 | 0.14 | healthy |
| `memory-full` | 215 | 0 | 10 | 1.02 | 0.24 | healthy |
| `absent-at-startup` | 0 | 0 | 0 | 0.34 | 0.10 | healthy |
| `incompatible` | 0 | 0 | 0 | 0.00 | 0.00 | ready, Redis rejected |
| `replicated-logout` | 0 | 0 | 0 | 0.00 | 0.00 | logout survived promotion |

Partition recovery times in `recovery_by_endpoint_ms` are measured from the
moment the relay resumed traffic.

| Endpoint | Recovery (ms) |
| --- | ---: |
| `public` | 125 |
| `deny` | 125 |
| `profile` | 127 |
| `jwt` | 116 |
| `api_key` | 116 |
| `oauth` | 133 |
| `userinfo` | 134 |
| `iam_ingress_deny` | 164 |
| `gateway_ingress_deny` | 166 |
| `oauth_refresh` | 173 |
| `account_flows` | 163 |
