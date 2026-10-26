# Gateway performance

Opt-in release workloads measure the gateway's latency, CPU, and memory costs
and verify that its work stays bounded. The recorded results below are a
regression reference from one development machine, not a production capacity
estimate or a latency target for deployments. To size a deployment, see
[Before sizing a deployment](#before-sizing-a-deployment).

## Reproduce

```bash
python3 benches/scripts/benchmark_gateway.py --seconds 3 --runs 3 --output /tmp/gateway.json
python3 benches/scripts/benchmark_gateway.py --seconds 3 --runs 3 --scenarios plain,context_signing,mixed_signing_short --output /tmp/signing.json
python3 benches/scripts/benchmark_gateway.py --seconds 15 --runs 1 --scenarios slow_mirror,status_failover,transport_failover,slow_failure,replay_saturation --output /tmp/sustained.json
python3 benches/scripts/benchmark_gateway_backends.py --verify --output /tmp/backends.json
python3 benches/scripts/benchmark_gateway.py --verify --seconds 3 --runs 3 --output /tmp/combined.json
cargo bench --locked -p ctx --bench internal_context_signing -- --quick
```

`benchmark_gateway.py` runs the ignored release tests `gateway_release_workloads`
(`src/gate/performance/`) or, with `--verify`, `gateway_combined_verification`.
It starts ephemeral localhost servers inside the test, uses public test RSA
material and an explicit HS256 session secret, and runs a fresh process per
repeat. It uses only the memory store: it does not read the application's
`.env`, start the application, or write application state. Omit `--scenarios`
for the complete matrix.

`benchmark_gateway_backends.py` runs `cluster_backend_round_trips`
(`src/data/performance.rs`) and, with `--verify`, the Redis gateway correctness
test `cluster_gateway_redis_integration` (`src/gate/tests/cluster.rs`). It
requires Docker and creates disposable Redis/PostgreSQL containers from images
already available locally (`--pull=never`), publishes random ports on localhost
only, runs migrations in its new database, and removes its containers on
completion. Neither script uses a deployment database.

Both scripts inherit build settings and need the Rust dependencies installed.
Run one deployment profile at a time, with other builds and workloads idle, and
compare results only across identical fixtures and the release profile.

## Hardware and workload

Recorded 2026-10-05: Apple M1 Pro (MacBookPro18,1), 10 CPUs, 16 GiB RAM,
macOS 27.0.1, Rust 1.98.0. Gateway, load generator, and upstream fixtures share
one process and **two Tokio worker threads**; internal-context signing runs on
two dedicated CPU threads. Two Tokio workers are few enough to expose blocking
work on the runtime; they do not model the application's default worker count.

Each scenario uses 16 closed-loop clients, a connection warmup, and three repeats
of three seconds. Responses are 1 KiB, except streaming responses (three 1 KiB
frames separated by 1 ms). Session auth uses actual HS256 verification and memory
store subject/revocation reads. Context signing uses actual RSA-2048 and fresh
per-attempt tokens. The mixed signing case splits clients equally between signed
upstream calls and short local responses, alongside two continuous 1 KiB streams
and two WebSockets echoing every 10 ms. Background sessions must stay alive and
exchange data until the workload ends.

Status failover discards a paced 64 MiB 503 response with a 20 ms discard deadline;
transport failover uses a TCP endpoint that closes accepted connections. Slow
failure adds a 20 ms header delay. Slow mirrors time out against one-second
upstream delays. Reload prepares and activates a new snapshot every 100 ms.

The local budget profile is primary concurrency 32, mirrors 4, replay body
64 KiB, aggregate replay memory 1 MiB, response headers 100 ms, discard 20 ms,
mirror lifetime 200 ms, and body idle 1 s; other time/size settings keep the
`runtime` defaults. Slow mirrors and replay saturation carry 64 KiB GET bodies.
Primary saturation lowers primary admission to 2; replay saturation lowers replay
memory to 128 KiB. Ingress/resource token buckets have one-million capacity and
refill to avoid rate throttling. The signing pool has 2 workers, 32 queued jobs,
and a 50 ms deadline to start. The raw results include the configuration and
connection counts.

## Measurement scope

Latency starts at the driver call. Header latency ends when response headers
arrive; body latency includes collecting the complete response. Nearest-rank
percentiles cover all attempts, including overloads where present; successful
throughput is reported separately. Status `0` is a client transport/body error,
not an HTTP status. Early rejection during a 64 KiB upload can close the client
connection before the driver receives HTTP 503; application counters keep the
rejection reason. High rejection throughput is not useful service capacity.

CPU is process CPU time divided by wall time, normalized to one core (200% means
two cores). RSS includes the driver, fixtures, metrics, and retained latency
samples; it is not a standalone gateway memory figure. Resource/task sampling
runs every 10 ms, RSS approximately every 100 ms, so brief peaks may be missed.
`tracked_tasks_peak` covers gateway-owned mirror/WebSocket tasks; total Tokio
tasks also include clients, server connections, the SDK, and fixtures. Signing
threads are outside the Tokio task count. Hard semaphore/channel bounds, queue
tests, and end-of-case assertions complement these sampled peaks: every case must
return all primary/mirror/replay permits and end with zero tracked gateway tasks
after cancellation and a drain interval.

Route tests scan to the last matching router at 1, 100, and 1,000 routers for
nine matcher types. Their figures are average scan times within batches of 100,
not end-to-end request percentiles. Backend measurements are 1,000 sequential
application calls after ten warmups, including serialization, pool, and local
Docker network overhead; they are separate from gateway CPU results and do not
establish cluster throughput.

## Results

Tables show the median of three runs (not a percentile of pooled percentiles).

### Signing under live I/O

Signing runs on a bounded worker pool, so signed requests do not hold the Tokio
workers that serve unrelated traffic. All requests succeeded and the long-lived
sessions stayed active. Body latency in milliseconds and successful aggregate
throughput:

| Case | Requests/s | p50 / p95 / p99 |
| --- | ---: | --- |
| Plain upstream | 28,197 | 0.518 / 0.898 / 1.051 |
| Context signing | 1,333 | 11.999 / 12.280 / 12.665 |
| Mixed, all requests | 39,532 | 0.199 / 0.406 / 6.126 |

**Short-request p99 in the mixed case is 0.430 ms** (runs: 0.430 / 0.429 /
0.440 ms); it was 5.6 ms when signing ran on the Tokio workers. The aggregate
throughput is mostly short local responses, not signed-request capacity: signed
attempts in that case run at about 1,322/s. Process CPU is about 391% because
signing and unrelated I/O progress concurrently on separate threads; the
signing-only case uses about 222%, plain about 193%.

### Complete gateway matrix

| Scenario | Successful requests/s | Body p50 / p95 / p99, ms | Header p99, ms | CPU % | RSS peak, MiB | Total / tracked task peak | Replay peak, KiB |
| --- | ---: | --- | ---: | ---: | ---: | --- | ---: |
| `plain` | 32,414 | 0.470 / 0.759 / 0.986 | 0.985 | 194 | 32.3 | 86 / 0 | 0 |
| `streaming` | 2,067 | 7.589 / 9.398 / 10.136 | 1.864 | 58 | 34.0 | 118 / 0 | 0 |
| `authenticated_hs256` | 29,331 | 0.519 / 0.825 / 1.063 | 1.061 | 195 | 36.4 | 86 / 0 | 0 |
| `context_signing` | 1,325 | 12.001 / 12.305 / 14.139 | 14.137 | 220 | 37.7 | 84 / 0 | 0 |
| `mixed_signing_short` | 44,224 | 0.181 / 0.356 / 6.101 | 6.099 | 395 | 43.5 | 80 / 2 | 0 |
| `mirror` | 26,464 | 0.580 / 0.876 / 1.163 | 1.162 | 193 | 46.6 | 100 / 4 | 384 |
| `slow_mirror` | 21,834 | 0.700 / 1.096 / 1.388 | 1.385 | 197 | 53.2 | 134 / 4 | 256 |
| `status_failover` | 659 | 24.369 / 26.160 / 27.688 | 27.685 | 43 | 54.4 | 152 / 0 | 1024 |
| `transport_failover` | 15,938 | 0.919 / 1.443 / 1.805 | 1.803 | 190 | 54.5 | 93 / 0 | 1024 |
| `slow_failure` | 341 | 46.697 / 49.503 / 54.272 | 54.267 | 37 | 53.5 | 132 / 0 | 1024 |
| `reload` | 31,843 | 0.472 / 0.805 / 1.028 | 1.027 | 193 | 53.8 | 87 / 0 | 0 |
| `live_streams_websockets_short` | 57,213 | 0.272 / 0.417 / 0.570 | 0.569 | 194 | 57.0 | 72 / 2 | 0 |
| `primary_saturation` | 95 | 0.180 / 0.341 / 0.454 | 0.453 | 191 | 57.6 | 56 / 0 | 0 |
| `replay_saturation` | 45 | 0.633 / 1.034 / 1.359 | 1.356 | 176 | 58.5 | 73 / 0 | 128 |

RSS, task, and replay columns show the maximum across repeats; the other columns
show medians of run-level values. Throughput varies between independent runs on
a shared development machine. Header and body latency diverge for streaming:
headers arrive early while the three frames complete later. Mirrors stay capped
at four; capacity skips occur even against healthy mirrors when 16 primaries
compete for four optional slots. Discard deadlines dominate status/slow failover
latency. Ordinary scenarios return only HTTP 200; the admission cases also reject
overload, and their percentiles are dominated by those fast rejections.

### Matcher costs

Last-match scans across 1,000 routers, median batch-average microseconds:

| Matcher | µs |
| --- | ---: |
| Exact path | 3.43 |
| Prefix path | 11.96 |
| Template path | 32.07 |
| Regex path | 8.91 |
| Host suffix | 96.91 |
| Header equality | 27.79 |
| Query equality | 81.12 |
| Cookie equality | 54.55 |
| CIDR | 7.95 |

Path, header, cookie, and host matchers borrow normalized values instead of
allocating copies, and source-IP networks are parsed once when the graph is
compiled. At these costs the router needs no index or request cache, and
priority and declaration order stay as configured.

## Signer microbenchmark

The Criterion RSA-2048 user+organization benchmark (`crates/ctx`), quick sampling,
same hardware:

| Case | Median time | Tokens/s |
| --- | ---: | ---: |
| Single token | 1.484 ms | 674 |
| 2 workers, batch 32 | 24.606 ms | 1,300 |
| 4 workers, batch 64 | 25.281 ms | 2,532 |

These are Criterion's median estimates (`estimates.json`), not its fitted slope.
The 4-worker case uses Criterion's Rayon pool, not the gateway's two-worker
queue: it shows CPU scaling only. End-to-end capacity comes from the workload
results above.

## Backend round trips

Disposable local `redis:latest` and `postgres:17-alpine` containers (image IDs in
the raw results), exercising the actual `RedisStore`, GCRA/quota limiters, and
`DbStore` calls with the application's default pools. PostgreSQL runs migrations
before measurement; the organization query deliberately misses.

| Application call | p50, ms | p95, ms | p99, ms |
| --- | ---: | ---: | ---: |
| `redis_session_get_1k` | 0.321 | 0.391 | 0.436 |
| `redis_revocation_exists` | 0.300 | 0.356 | 0.501 |
| `redis_gcra` | 0.318 | 0.440 | 0.721 |
| `redis_quota` | 0.310 | 0.369 | 0.461 |
| `postgres_select1` | 0.670 | 0.864 | 1.348 |
| `postgres_org_lookup_miss` | 0.691 | 0.897 | 1.347 |

These round trips add latency outside the in-memory gateway CPU work. Docker's
local VM/network and the default pools matter; remote stores, contention,
failover, and full application authentication are separate integration concerns.
There is no cluster throughput claim here.

## Sustained bounded work

Fifteen seconds per case, one continuous process:

| Case | Successful requests/s | Gateway task peak | Total Tokio task peak | Replay peak, KiB | RSS peak, MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| Slow mirror | 17,927 | 4 | 132 | 256 | 49.8 |
| Status failover | 698 | 0 | 154 | 1,024 | 58.2 |
| Transport failover | 13,443 | 0 | 94 | 1,024 | 58.4 |
| Slow failure | 354 | 0 | 132 | 1,024 | 58.5 |
| Replay saturation | 45 | 0 | 73 | 128 | 58.8 |

Slow mirrors recorded 268,326 capacity skips and 596 header timeouts; optional
work stayed at four tasks and 256 KiB. Status failover recorded 10,469 bounded
disposal timeouts, slow failure 5,333. Replay saturation recorded 257,802 server
rejections with `kind=replay_memory`, plus client upload/transport failures,
while successful traffic continued at 45/s. Every case ended with all resource
permits returned and zero tracked gateway tasks. These runs, the hard bounds, and
the cancellation tests establish bounded work; RSS includes measurement arrays and
fixture tasks, so it is not a long-term gateway-only memory curve.

## Client pool reuse

`RuntimeSnapshot::client()` reads an immutable map and clones the prepared client;
it takes no global write lock. Transports are built only during preparation and
published with the graph. Steady plain/auth traffic used 17 upstream TCP
connections for roughly 85,000–100,000 requests per case. Reload deliberately
rebuilds transports: 30 generations per case produced roughly 450–470
connections. Pinned old generations drain while new requests use the new pool,
so pools are reused within a configuration generation, not across generations.

## Budget sizing

For this 16-client profile, primary admission 32 leaves room for the four
long-lived sessions. Four slow mirrors retain at most 256 KiB of shared 64 KiB
replay bodies. A 128 KiB replay pool admits at most two full reservations, and a
primary budget of two against 20 ms upstream work admits roughly 100 successful
requests/s. The benchmark exercises these rejection paths instead of hiding their
costs behind an unbounded queue.

Size primary admission from expected concurrency (arrival rate × full response
lifetime), including streams and WebSockets. Reserve replay capacity by the
configured per-body cap, not average bytes: the 64 MiB / 2 MiB defaults allow 32
simultaneous full reservations. Mirror concurrency and lifetime set both retained
request memory and maximum optional work: when every job reaches the 200 ms
lifetime, four slots complete about 20/s; in this profile the 100 ms header
deadline expires first, giving about 40/s. Higher coverage needs capacity backed
by measured latency and memory headroom.

The process defaults stay at primary 1,024, mirrors 64, replay body 2 MiB, and
replay pool 64 MiB: local micro-workloads do not justify other ceilings for every
deployment. Use the smaller measured profile for comparable workloads and tune
timeouts to the upstream's tail latency. The signing defaults protect unrelated
I/O; more queued work does not add CPU throughput. Watch signing queue
wait/rejections, replay skips, and header/body latency together before adjusting
limits (instruments in [config-v1.md](config-v1.md); signing in
[internal-context-operations.md](internal-context-operations.md)).

## Combined verification

`benchmark_gateway.py --verify` runs three fresh-process release repeats of 16
clients for three seconds: four status failover, eight slow mirror with 64 KiB
bodies, and four short local responses, alongside two live HTTP streams and two
WebSockets. During traffic it performs ten configuration reloads and one mTLS
reload, rejects missing and invalid client-key files without replacing the active
snapshot, disconnects one HTTP and one WebSocket client, and sends primary/replay
overload probes (`503`). Shutdown cancels the active mirrors and remaining live
connections.

| Repeat | Successful traffic requests | Short header p99, ms | Mirror / tracked task peak | Replay peak, KiB | Final permits / tasks |
| --- | ---: | ---: | --- | ---: | --- |
| 1 | 84,835 | 0.620 | 4 / 6 | 512 | 32 primary, 4 mirror, 1 MiB replay / 0 |
| 2 | 85,157 | 0.609 | 4 / 6 | 512 | 32 primary, 4 mirror, 1 MiB replay / 0 |
| 3 | 86,555 | 0.596 | 4 / 6 | 512 | 32 primary, 4 mirror, 1 MiB replay / 0 |

Traffic totals exclude setup and the overload probes. Each repeat verifies that
the initial snapshot stays alive while its sessions run and is released on drain,
with all active-resource counters back to zero. The real metrics exporter checks
bounded timeout/capacity/reload labels and replay bytes. `stargate.gateway.duration`
and the upstream duration stop at response headers;
`stargate.gateway.transfer.duration` records upload, guarded response body, and
accepted WebSocket lifetimes with completion/error/timeout/cancellation/drop
outcomes.

`benchmark_gateway_backends.py --verify` runs twelve Redis correctness checks:
pre-auth admission with a counting authentication boundary, allowance retention
across reloads, subject rate and weighted quota, unknown selected limits,
admission before store overload, Redis wrong-type recovery, store deadline
recovery, atomic GCRA/token-bucket/quota allowances across two connection pools,
and shutdown. Identity injection is test only; limiter and store operations are
real.

## Before sizing a deployment

Supply the hardware and runtime worker count, sustained arrival rate,
request/response size distribution, live connection count, mirror coverage, store
topology, and latency/error targets. Repeat the workloads under those conditions
with external load generation, exported telemetry, and realistic upstream tails.
The local two-worker baseline is not a deployment SLA.
