# Redis failure bench

`benches/scripts/redis_failures.py` runs a disposable cluster-profile Stargate
(PostgreSQL and Redis containers on localhost) under a light mixed load and
injects one Redis failure per scenario. Every scenario asserts the documented
behavior from [health probes](health-probes.md) and the API overview; it does
more than check that the process stays alive. The report lists the responses
that did not match, the duration of the disturbance and the time to recover.

The load sends three requests in a loop: a gateway route whose rate limit fails
open (`/bench/public`), the same route with `on_store_error: deny`
(`/bench/deny`), and `GET /account/profile`, a store read on every request.
`x-ratelimit-limit` identifies a limit check that actually reached the store, so
a fail-open response is a `200` without allowance headers.

## Scenarios

| Scenario            | Injection                                                                                                                                                     | Documented behavior asserted                                                                                                                                                                                                                                                                                                                                                                      |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `restart`           | Stop and start the Redis container (AOF on, state survives).                                                                                                  | Limits fail open: `/bench/public` returns `200` without allowance headers, `/bench/deny` returns `503 gateway.limiter_unavailable`, `/account/profile` never returns `200`, `/readyz` stays `200`, `/admin/health` reports Redis `unhealthy` and `limits.storeDegraded`. After the restart the same session works, limits are enforced again, `storeDegraded` clears and Stargate never restarts. |
| `empty`             | Stop and start Redis with persistence off.                                                                                                                    | The session key is gone: the old access token gets `401 auth.token_invalid` from `/account/profile`, the old refresh token gets `401`, and a fresh sign-in works. Limits and health recover; there is no permanent error.                                                                                                                                                                         |
| `partition`         | A localhost TCP relay forwards to Redis; `SIGSTOP` on the relay blackholes every packet while its sockets stay open. Held past the 5s store response timeout. | Same fail-open and fail-closed split as `restart`, with `/admin/health` also reporting overall `unhealthy` and `limits.storeDegraded`; readiness does not follow Redis. `SIGCONT` restores every check without restarting Stargate.                                                                                                                                                               |
| `memory-full`       | Redis starts with `maxmemory 2mb` and `noeviction`; filler keys are written, then `maxmemory` is pinned below the stored data so every write is rejected.     | Reads keep working (`/account/profile` returns `200`), limit checks fail open or closed as configured, sign-in fails closed (`5xx`) because the session write is rejected, `PING` keeps the Redis component `healthy` while `limits.storeDegraded` is true and the overall status is `degraded`. Freeing the filler keys and restoring `maxmemory` restores writes without a restart.             |
| `absent-at-startup` | Redis listens on the chosen port, then the container is removed before Stargate starts.                                                                       | Store initialization runs before the listener opens, so the process exits non-zero and names Redis; it never answers HTTP. The expectation is `STARTUP_WITHOUT_REDIS_FAILS` at the top of the script: when startup without Redis is supported, invert it and assert readiness with the Redis component reported unhealthy.                                                                        |

Cluster and Sentinel topologies are out of scope; only a single Redis is
exercised.

## Run locally

```bash
cargo build --features cluster
python3 benches/scripts/redis_failures.py \
  --redis-image redis:7.4-alpine \
  --postgres-image postgres:17-alpine
```

Requirements: Docker, a `target/debug/stargate` built with the `cluster`
profile, and Redis >= 7.4 (hash-field expiry). `--scenarios` selects a subset,
`--output` writes the JSON report. Containers are disposable, bind to localhost
only and are removed on exit. The script exits non-zero at the first scenario
whose behavior does not match.

The project's other Redis suite is `benches/scripts/test_redis_prefix.py`; it
verifies the key prefix, the production ACL and the session/limiter/relay smoke
test. Run it with the same images.

## CI

`.github/workflows/redis-failures.yml` runs on pull requests that touch
`crates/store`, `crates/lim`, `src/data/store`, the session and authentication
code, the gateway limits and policies, the bench scripts or this guide, and on
manual dispatch:

```bash
gh workflow run redis-failures.yml --ref main
```

The job builds the cluster profile, runs the prefix/ACL smoke test, then this
bench with `redis:7.4-alpine` and `postgres:17-alpine`.

## Recorded results

Recorded 2026-11-17 on Apple M1 Pro (10 CPUs), macOS 27.2.1, Docker 29.9.3,
`target/debug/stargate` from `test/redis-failure-bench`, running the command
above. Durations are indicative: `disturbance_seconds` runs from the injection
to the end of the fault, and `recovery_seconds` from there to the documented
healthy state (limits enforced, sessions usable, `/admin/health` healthy).
`unexpected` counts load responses that did not match the phase expectation and
is zero for a passing run.

| Scenario            | Requests | Unexpected | Disturbance (s) | Recovery (s) | Final state             |
| ------------------- | -------- | ---------- | --------------- | ------------ | ----------------------- |
| `restart`           | 165      | 0          | 8.65            | 0.73         | healthy                 |
| `empty`             | 172      | 0          | 6.11            | 3.31         | signed-out then healthy |
| `partition`         | 159      | 0          | 8.54            | 0.07         | healthy                 |
| `memory-full`       | 214      | 0          | 1.04            | 0.20         | healthy                 |
| `absent-at-startup` | 0        | 0          | 9.49            | —            | failed at startup       |

The `restart` and `empty` disturbances include the deliberate
`/account/profile` probe, which waits for the store's 5s response timeout
before failing closed, and a one-second hold under load so the sampled windows
stay meaningful; without them the outage itself is under a second. The
`partition` disturbance is intentionally longer than that timeout so the same
probe has time to fail; shorter blackholes only exercise the 250ms ingress and
policy deadlines. The `absent-at-startup` duration is the connection retry
time before the process exits, not a timeout of the bench.
