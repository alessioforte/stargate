# Cluster sizing

This reference run measures three cluster replicas behind Traefik, with real
PostgreSQL and Redis over verified TLS and an external load generator. It is a
starting point for budgets and regression comparisons, not a production throughput
promise. Re-run on the intended hardware with representative identities, routes
and backends. Workload mixes, expensive historical password hashes, slow clients,
MFA, network distance and observability overhead change these numbers.

## Reference environment

Recorded on 2026-11-03, from source based on `bf62a08` with bounded password hashing
and strict pool settings. [Machine-readable results](../benches/results/cluster-sizing.json)
include the changed Rust-source fingerprint, all 38 stages, per-replica CPU/memory
samples, SQL pool usage, hashing activity, error counts and Redis latency samples.

| Component | Configuration |
| --- | --- |
| Hardware | Apple M1 Pro; Docker VM: 10 vCPUs, about 7.65 GiB RAM, ARM64 |
| Topology | One control-plane node, three workers, three Stargate replicas; singleton SQL, Redis, backend and collector fixtures |
| Runtime | Release build with Rust 1.99.1; k3s 1.34.3; Traefik 3.5.1 |
| Dependencies | PostgreSQL 17.6, Redis 7.4.2, Collector Contrib 0.111.0, Node.js 22.18.0 load client/backend |
| Stargate resources per pod | Request 250m CPU / 512 MiB; limit 2 CPUs / 1 GiB |
| Password work | Concurrency 2; Argon2id 19,456 KiB, 2 iterations, parallelism 1 |
| Pools | SQL maximum 20; Redis connections 4, per replica |
| Transport | Verified TLS to Traefik; HTTP backend over mTLS with signed internal context |
| Observability | Traces/logs/metrics enabled; OTLP debug collector; metric export every second |
| Generator | One external Docker client/IP; open-loop arrivals, 15-second stages, at most 512 in-flight operations |
| Capacity-test admission | Stargate ingress burst 10,000, refill 1/ms; Traefik backstop disabled; policy ceilings raised |

The local image build explicitly selected Rust 1.99.1. The repository's pinned
1.96.0 build encounters the existing workspace `chrono` feature-declaration issue
before compilation; that default build was not used for these results.

CPU comes from metrics-server averaging windows, typically 10–20 seconds; adjacent
stages can overlap in those samples. SQL/activity gauges are sampled at one second,
so a brief peak can be missed. No host compilation ran during the capacity ramps.
The generator's initial unconditional zero-delay sleep imposed an artificial
1 ms floor; that gateway sample was excluded and its high-rate stage rerun after
fixing the timer. `generator.capacity` counts operations **not sent**, separately
from actual server errors. Generator schedule lag remains in the artifact.

## Workloads and observed ceilings

The table shows the last tested stage without errors and below the p95 threshold:
200 ms for HTTP/WebSocket, 1,500 ms for password login/OAuth. It is not an isolated
hardware maximum. An operation is one gateway request, one password login, one
complete OAuth flow, or a discovery **and** JWKS pair. WebSocket latency is frame
round-trip time, with one message per connection per second.

| Workload | Last clean offered stage | Completed operations/s | p50 / p95 / p99, ms | First failing offered stage |
| --- | --- | --- | --- | --- |
| JWT gateway | 500/s | 499.91 | 4.49 / 12.31 / 29.32 | 1,000/s: 68 `503 gateway.overloaded`, 194 not dispatched |
| JWT gateway + rate limit + quota | 250/s | 250.02 | 4.66 / 8.15 / 14.52 | 500/s: 145 `503 gateway.overloaded`, 1,185 not dispatched |
| Password login | 100/s | 92.64 | 129.47 / 1,024.10 / 1,209.64 | 200/s: 55 `503 iam.overloaded`, 978 not dispatched |
| Authorize → code → token → refresh | 400/s | 399.64 | 19.47 / 43.89 / 123.28 | 800/s: 4,327 `429 iam.ingress_rate_limit_exceeded`, 573 not dispatched |
| Discovery + JWKS | 500/s | 500.00 | 1.70 / 2.96 / 6.26 | 1,000/s: 3,586 `429 iam.ingress_rate_limit_exceeded`, 75 not dispatched |
| WebSocket | 500 connections | 466.39 messages/s, including setup | 16.19 / 26.70 / 32.83 | 1,000 concurrent handshakes: 15 HTTP 503 handshake failures |

OAuth uses a preauthenticated native session, a first-party public client with
PKCE S256 and `openid offline_access`, then exchanges and rotates the refresh
token. Password login creates a new native session each time, using the correct
password without MFA. Browser rendering, consent and email delivery are excluded.
WebSockets open concurrently: the failing stage measures handshake pressure as
well as active sessions; it is not a hard steady-connection limit.

Discovery performs two HTTP calls per operation; OAuth performs three. Their
high-rate ceilings include the shared per-IP ingress allowance, and the short
OAuth stages can consume burst capacity. Do not project the 400-flow/s sample to
an indefinite single-IP rate. Likewise, no conclusion about unlimited public
endpoint throughput follows from a rate-limited result.

The password overload stage observed at most two executing hash jobs per replica,
about 5.06 seconds at p95 and a sampled pod-memory peak of **466.2 MiB**, below the
1 GiB limit. Overload did not lock the account: subsequent valid login succeeded.
Cancellation tests additionally verify that abandoned requests retain their
blocking-work permits until computation finishes. The active Argon2 allocations
alone are only `2 × 19456 KiB = 38 MiB`: allocator retention, pending requests,
JWT/session handling and telemetry require substantial additional headroom.

## PostgreSQL connections

Budget capacity, not just observed connections:

```text
(server replicas + rollout surge) × DB_POOL_MAX
  + concurrent Job pools × Job DB_POOL_MAX
  + other clients
  ≤ max_connections - reserved/headroom
```

The PostgreSQL notification listener reserves one connection **inside** each
server pool; do not add a second connection outside the pool. Migrations, bootstrap
and configuration-import processes have separate pools. Include old replicas
while a rollout is active and any overlapping maintenance/administration work.

With the defaults and three replicas, servers alone can use `3 × 20 = 60`.
Conservatively allow one surge replica and two Job pools: `4 × 20 + 2 × 20 = 120`.
A PostgreSQL maximum of 150 with 20 connections held back leaves room for that
budget. The production overlay explicitly keeps pool maximum 20; the highest
sampled per-replica usage was 10, including the listener. Sampling can miss peaks,
so the run does not justify shrinking the pool to its sampled maximum.

Do not enable HPA without recalculating this budget. For example, six replicas
and the default 25% surge can mean eight server pools; with two Jobs the ceiling
is `8 × 20 + 2 × 20 = 200`, before headroom. A maximum of 240 with 20 held back
covers that example. Set the CNPG parameter and connection reservations in the
operator's database manifests; the Stargate chart does not configure CNPG.

## Redis connections and memory

`REDIS_POOL_SIZE` defaults to 4: three replicas maintain 12 selected-store
connections. The store, traffic limiter and audit relay share these pools;
diagnostics and external consumers add their own connections. Test Redis TLS
latency under load, not just TCP reachability. The reference artifact contains
continuous min/max/mean/sample-count observations from the Redis TLS endpoint.

Measured `MEMORY USAGE` on real stored MessagePack records:

| Record | Bytes |
| --- | ---: |
| Native subject | 112 |
| Native refresh rotation | 128 |
| One-session user index | 176 |
| Directory share per session | about 182 |
| Native session total | about 598 |
| OAuth refresh family | 416 (200 sampled records) |
| Pending authorization code | 392 |

Assume one native session, one OAuth family and one pending authorization code
per active user: about **1,406 bytes/user** of measured record storage. That gives
**13.4 MiB for 10,000 users** or **134.1 MiB for 100,000 users**. Reserving twice
that data estimate gives about 27 / 269 MiB, **before** Redis baseline memory,
additional key/expiry dictionaries, client/output buffers, replication, AOF
rewrite/fork overhead and operating-system headroom. The run's Redis RSS/used
memory ratio was about 1.71; it is not a universal fragmentation factor.

User count alone is insufficient: multiply by sessions per user, applications
with refresh families, pending-code issuance rate × TTL, revocation/login-guard
keys and audit retention. Linked clients, claims and scopes change record sizes.
Keep audit-stream capacity and consumer outages in the separate Redis budget;
never derive the server memory limit directly from this small-session estimate.

## Pod resources and scaling

The production overlay keeps request 250m CPU / 512 MiB and limit 2 CPUs / 1 GiB,
with `PASSWORD_HASH_CONCURRENCY=2`, SQL maximum 20 and Redis pool 4. These are
initial values validated by the reference run, not an SLO guarantee. Long soaks
and larger user profiles are still required on the target installation.

The CPU request is appropriate for a primarily gateway workload in this sample.
For sustained password-heavy traffic, reserve more CPU: the failed login stage
reached about 1.8 cores per pod, and the clean 100-login/s stage about 0.95 cores.
Concurrency must fit both CPU and memory; `available_parallelism()` alone is not
a memory plan. Leave room beyond `concurrency × ARGON2_MEMORY_KIB`, including
higher historical hash costs used during verification/history checks.

For CPU-bound authentication, start evaluating HPA around 70% CPU utilization
with at least three replicas, after checking the PostgreSQL/Redis/backend budget.
HPA CPU percentage is relative to **requests**, not limits: 70% of 250m is 175m.
An IAM-heavy deployment may need a higher request as well as more replicas.
Scaling gateway replicas does not fix an overloaded backend, collector or Redis.
Track `stargate.iam.password_hashing.rejections` excluding `rehash_skipped`, pool
utilization, admission errors and p95/p99 while tuning.

## Ingress, NAT and the independent limiter

Stargate's example ingress allowance is shared across replicas: burst 100 and
10 HTTP requests/s **per client IP**. Many users behind one corporate/mobile NAT
share it. Raise it for a known workload together with its backstop and resource
budget; account for the several requests in an OAuth or login-page flow.
Configure `trustedProxies` for the actual Traefik peer networks so Stargate sees
the resolved client instead of charging everybody to the proxy address. Do not
trust arbitrary Internet-supplied forwarding fields.

The chart's `traefik.rateLimit` configuration applies to every identity, API,
admin and gRPC router, before the admin allow-list. Its default is disabled; the
production example uses **20/s, burst 200**, above Stargate's example allowance.
Traefik's memory state is **per IP, per router, per Traefik instance**: the same
Middleware name does not share its bucket across routers. In the measured 3.5.1
release, exhausting two tokens on the identity route still left two on the API
route from the same client. This follows from Traefik's
[per-handler construction](https://github.com/traefik/traefik/blob/v3.5.1/pkg/server/middleware/middlewares.go)
and [local bucket allocation](https://github.com/traefik/traefik/blob/v3.5.1/pkg/middlewares/ratelimiter/in_memory_limiter.go).
The chart creates R routers as follows:

- Identity: 1.
- Ordinary API: 1 when `apiHosts` is nonempty, combining all API hosts.
- Admin: 1 when `adminAllowList.enabled` is true.
- gRPC: 1 per API host when `grpc.enabled` is true.

The admin allow-list still rejects outside IPs with `403`: its router contributes
only to the theoretical maximum for those IPs and never grants admin access.
For N Traefik instances, one IP can reach **average × N × R** in total, but only
**average × N** for a group of endpoints on one router (for example login and
token endpoints on the identity router). Rates are per configured `period`;
aggregate burst bounds similarly multiply `burst` by N and R.

Keep both `average / period` and `burst` above Stargate's ingress rate and burst.
When increasing Stargate's ingress allowance, increase these backstop values as
well. These are per-IP safeguards: an attack from many IPs can exceed any such
limit and requires network-level or CDN protection.

Established WebSocket frames consume no HTTP admission tokens.

The outage experiment used one identity route and one Traefik instance at
100 discovery/JWKS operations/s for 20 seconds, with Stargate 10/s + burst 100,
250ms store-check timeout and `on_store_error: allow`:

- Redis healthy: 300 operations failed with Stargate's IAM ingress code and
  1,648 with a Traefik 429; 52 complete operations succeeded.
- Redis actually stopped: zero Stargate ingress-limit denials, 1,992 Traefik 429
  failures and eight complete operations succeeded. Every operation performs two
  requests; partially successful operations still count as failures.
- Redis restarted successfully and a public IAM request returned 200.

This tests the independent limiter during a bounded outage, not availability
through every outage. Readiness checks SQL and lifecycle state; Redis does not
gate readiness. Fail-open traffic checks do not relax session/OAuth security-store
requirements. The disposable Redis fixture had persistence disabled; its restart
lost sessions. Production persistence and failover need their own verification.

## Repeating the workload

Use a disposable environment and an external client. The driver
[benchmark_cluster.mjs](../benches/scripts/benchmark_cluster.mjs) uses Node built-ins
for HTTP; the WebSocket case requires `ws` installed alongside it (the reference
fixture uses 8.22.0). TLS verification is always enabled. Credentials/configuration
come from stdin and output contains aggregate measurements, never tokens or bodies.

Create a private JSON input outside the repository, mode 0600, containing:

```json
{
  "scenario": "gateway",
  "identityOrigin": "https://identity.example.test",
  "apiOrigin": "https://api.example.test",
  "caFile": "/secure/test-ca.crt",
  "token": "<short-lived native session token>",
  "username": "<test account>",
  "password": "<test password>",
  "oauthClientId": "<first-party public test client>",
  "redirectUri": "https://client.example.test/callback",
  "rates": [5, 25, 100, 250, 500, 1000],
  "durationSeconds": 15,
  "latencyThresholdMs": 200
}
```

```sh
node benches/scripts/benchmark_cluster.mjs < /secure/workload.json > workload-results.jsonl
```

Choose `gateway`, `policies`, `login`, `oauth`, `discovery`, or `websocket`.
`gatewayPath`, `policyPath` and `websocketPath` override the default `/benchmark/`
paths. Install matching authenticated gateway routes; the policy route needs both
rate-limit and quota policies with ceilings appropriate for the experiment.
For OAuth, provision the redirect URI, authorization-code/refresh grants, the two
scopes and explicit first-party attribute; authorize uses the supplied session.
For WebSockets, rates are connection counts, with one message/s per connection.
The driver stops on an error, p95 threshold breach or excessive generator lag.

Raise client capacity with `maxInFlight` only when the generator has adequate
resources. Record server errors separately from `generator.capacity` skips.
Collect per-pod CPU/working-set memory with the metrics-server window, and export
SQL-pool/hash-activity gauges at a suitably short OTLP interval. Measure Redis
latency continuously during the workload (for example `redis-cli --latency --csv`
over verified TLS), with authentication supplied privately rather than in arguments.

For the outage experiment, restore ordinary ingress/backstop limits, measure the
public scenario, then stop only the disposable Redis deployment and repeat that
same workload. Record readiness alongside results, restore Redis in a finally/trap
handler, check recovery, and remove the environment and generated credentials.
The final stored artifact contains sanitized measurements; raw credentials and
private test state must never be committed.
