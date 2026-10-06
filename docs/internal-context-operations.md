# Internal Context Operations Runbook

Status: Stargate version 1 operations and release runbook.

This runbook covers functionality owned by Stargate. Microservice integration
is documented in
[`internal-context-consumer-guide.md`](internal-context-consumer-guide.md).
Real upstream state and enablement evidence belong to the responsible
deployment or release system. External dashboards and consumer telemetry are
not provisioned by this repository.

Delegation, replay caches, JWE, and new transports are not operational features
of version 1.

## Runtime prerequisites

Stargate loads the internal signer and public JWKS at process startup. Defining
any `INTERNAL_CONTEXT_*` variable selects explicit configuration; missing or
incompatible material fails startup. A gateway graph containing
`internal_context` also fails startup or hot-reload preflight when no valid
signer is available.

| Input | Operational rule |
| --- | --- |
| `INTERNAL_CONTEXT_ALGORITHM` | Must be `RS256` |
| `INTERNAL_CONTEXT_ISSUER` | Stable exact issuer; changing it is a coordinated consumer migration |
| `INTERNAL_CONTEXT_KID` | Must identify the active private key and a matching public JWK |
| `INTERNAL_CONTEXT_PRIVATE_KEY_PATH` | Dedicated RSA private key, at least 2,048 bits |
| `INTERNAL_CONTEXT_JWKS_PATH` | Public-only JWKS containing the active key and optional rotation overlap |
| `INTERNAL_CONTEXT_TTL_SECS` | Normally 30; maximum 60 |
| `INTERNAL_CONTEXT_CLOCK_SKEW_SECS` | Normally 5; maximum 30 |
| `INTERNAL_CONTEXT_JWKS_CACHE_MAX_AGE_SECS` | Published JWKS cache age; normally 60 |
| `INTERNAL_CONTEXT_SIGNING_WORKERS` | Fixed CPU workers; default 2, range 1–64 |
| `INTERNAL_CONTEXT_SIGNING_QUEUE_CAPACITY` | Maximum waiting jobs; default 32, range 1–4,096 |
| `INTERNAL_CONTEXT_SIGNING_QUEUE_TIMEOUT_MS` | Deadline to start signing; default 50 ms, range 1–60,000 |

## Initial key generation

Generate the initial private key and matching public JWKS explicitly:

```bash
cargo run --features edge -- internal-context generate-key \
  --output-dir .stargate/internal-context \
  --kid stargate-internal-current
```

The command uses operating-system randomness and creates a 2,048-bit RSA
private key in PKCS#8 PEM plus a public-only RS256 `jwks.json`. It refuses to
overwrite either output. Use `--bits 3072` or `--bits 4096` when the deployment
policy requires a larger modulus; accepted values are 2,048 through 8,192.

The command prints the matching `INTERNAL_CONTEXT_*` paths but never prints the
private key. Server startup only loads and validates provisioned material; it
does not invoke this command or silently replace a missing key.

Use UTC clock synchronization on Stargate and every consumer. Alert on failed
NTP synchronization or material drift greater than the configured skew. Do not
increase skew to hide an unhealthy clock.

The private-key directory should be owned by the Stargate runtime identity with
mode `0700`; the private key should be read-only to that identity with mode
`0600`. In containers, mount it from the deployment secret store as a read-only
file. Do not place private members (`d`, `p`, `q`, `dp`, `dq`, `qi`, `oth`,
`k`, or `key`) in the JWKS. Restrict backups, CI output, shell history, and
crash dumps as signing-key material.

Use TLS for every internal hop and prefer mTLS where the environment supports
it. Network policy must prevent clients from bypassing Stargate and reaching a
protected consumer directly. A signed context provides integrity and
authenticity, not confidentiality or network admission.

## Stargate telemetry contract

OpenTelemetry export uses the existing `OTEL_*` configuration. Metric and trace
attributes intentionally exclude token, request id, subject, organization,
path, audience, key id, and all decoded claims.

| Instrument | Type | Purpose |
| --- | --- | --- |
| `stargate.gateway.internal_context.issues` | Counter | Issuance/preparation outcomes |
| `stargate.gateway.internal_context.signing.duration` | Histogram in milliseconds | RS256 worker execution time, excluding queue wait |
| `stargate.gateway.internal_context.queue.duration` | Histogram in milliseconds | Queue wait for jobs that started, labeled by service and dispatch kind |
| `stargate.gateway.internal_context.token.size` | Histogram in bytes | Compact token size when a token was produced |
| `stargate.config.reloads` | Counter | Gateway configuration reload outcome, including preflight rejection |

Internal-context instruments use only these bounded dimensions:

- `stargate.service`: logical service name from the compiled gateway graph;
- `stargate.dispatch.kind`: `primary`, `shadow`, or `unknown` for an invariant
  failure before a dispatch kind is available;
- `stargate.outcome`: `success` or `failure`; and
- `stargate.reason`: `none`, `draft`, `runtime`, `sanitization`, `trace`,
  `binding`, `claims`, `time`, `signing`, `size`, `header`, `attempt`,
  `signing_queue_full`, `signing_queue_timeout`, or `signing_workers_unavailable`.

Successful issuance emits a debug event with logical service, dispatch kind,
signing duration, queue wait, and token byte size. Failure emits a warning with the same
safe fields that exist at the point of failure. Neither event contains the
compact token, payload, actor, organization, request path, audience, or key id.

### Recommended alerts and queries

Collectors may normalize dots in metric and attribute names. Adapt these
PromQL-style examples to the names exported by the deployed collector.

Any sustained issuance failure is actionable:

```promql
sum by (stargate_service, stargate_dispatch_kind, stargate_reason) (
  rate(stargate_gateway_internal_context_issues_total{
    stargate_outcome="failure"
  }[5m])
) > 0
```

Alert on a rejected gateway configuration reload:

```promql
sum(rate(stargate_config_reloads_total{
  stargate_config_kind="gateway_config",
  stargate_outcome="error"
}[5m])) > 0
```

Track signing latency and token-size headroom per logical service. Investigate
a sustained p99 signing regression or a token-size percentile approaching the
4,096-byte contract limit. Do not add subject, organization, request, path,
audience, or `kid` labels to make an alert more specific.

Consumer owners should separately expose bounded counters for presentation and
verification category, JWKS refresh outcome, trace mismatch, and requests
missing context at a protected boundary. Use a small fixed reason taxonomy such
as presentation, signature/key, issuer, audience, request binding, time, and
internal failure; key ids, tokens, signatures, claims, and header values must
not be labels or log fields.
Alert consumer owners on:

- any unexpected missing-context request;
- sustained signature, issuer, audience, method, path, request-id, or time
  rejection;
- failed JWKS refresh, especially with an expired last-known-good cache;
- clock-health failure or time-category spikes; and
- a trace-mismatch increase.

## Planned key rotation

Key files are process-start snapshots. Changing files or environment variables
does not replace the active in-memory signer. Use a process restart or rolling
deployment for each stage.

1. Generate a new RSA key and public JWK under a separate output directory:

   ```bash
   cargo run --features edge -- internal-context generate-key \
     --output-dir .stargate/internal-context/next \
     --kid stargate-internal-next
   ```

   Store the new private key in the deployment secret system. The generator
   does not modify the active JWKS.
2. Build a JWKS containing both the current and new public JWK. Validate that
   the current `INTERNAL_CONTEXT_KID` still matches the current private key.
3. Deploy or restart every Stargate replica with the current signer and the
   overlapping JWKS. Confirm
   `/.well-known/stargate-context-jwks.json` exposes both public keys and no
   private members.
4. Allow consumer caches to observe the overlap. Each consumer owner confirms
   an unknown-`kid` refresh accepts a test token signed with the new key while
   the old key still verifies.
5. Roll Stargate to the new private key and new `INTERNAL_CONTEXT_KID`, keeping
   both public keys in the JWKS. During a rolling deployment, both old and new
   signers may be live, so every replica must publish the same overlap.
6. After the last old signer stops, wait at least token TTL + maximum consumer
   clock skew + maximum downstream JWKS cache age.
7. Remove the old public JWK and roll/restart Stargate again. Confirm new tokens
   work and old tokens fail with an unknown key.

The repository drill uses two distinct RSA keypairs and can be repeated with:

```bash
cargo test --features edge rotation_drill_accepts_both_keys_before_retiring_the_old_key
```

Do not reuse OAuth/OIDC keys and do not overwrite the current private key
before the overlapping JWKS is serving everywhere.

## Fail-closed recovery drill

Run this before release and after changes to signing, replay, failover, or
configuration reload:

```bash
cargo test --features edge reload_candidate_failure_occurs_before_activation
cargo test --features edge issuance_failure_contacts_no_upstream
cargo test --features edge internal_context_failure_never_fails_over
python3 benches/scripts/benchmark_gateway.py --verify --seconds 3 --runs 3 --output /tmp/gateway-verification.json
python3 benches/scripts/benchmark_gateway_backends.py --verify --output /tmp/backend-verification.json
```

The drill proves:

- an invalid candidate fails preflight before graph activation;
- signing/preparation failure sends no request to the selected upstream; and
- an internal-context failure does not advance to an unsigned failover.

The opt-in script checks add simultaneous reload, TLS file rejection/recovery,
failover, mirror pressure, live HTTP/WebSocket traffic, disconnect, and shutdown.
They verify separate header/body lifetime metrics and zero retained gateway
resources. The backend script uses only its disposable localhost Docker stores
and checks actual Redis admission, rate/quota charging, and failure recovery.
See [gateway-performance.md](gateway-performance.md) for scope and recorded results.

The release tests live in `src/api/gateway/performance/`; Redis gateway
correctness lives in `src/api/gateway/tests/cluster.rs`, and backend workloads
live in `src/etc/performance.rs`. The runner selectors above are unchanged.
Signed dispatch and its tracing target belong to
`stargate::api::gateway::upstream::internal_context`; attempt spans belong to
`stargate::api::gateway::upstream::attempt`. Use the current namespaces for
module-specific log filters. The span names and metric instruments remain
stable through the gateway/shared-support refactor.

In an environment drill, point a test route at a capture service, then stage a
mismatched `kid`, private key, or JWKS in a replacement deployment. Confirm the
replacement fails readiness/startup, the current compatible deployment remains
active, the capture service receives no failed dispatch, and issuance/config
failure telemetry fires without token data.

Recovery choices are limited to:

1. restore the last compatible key/JWKS/configuration and restart or roll the
   failed Stargate deployment; or
2. disable the affected route until a compatible gateway and consumer pair is
   restored.

Never recover by forwarding an original bearer token, API key, Stargate JWT
cookie, or authentication query parameter. Never remove the consumer's
fail-closed check while leaving the route reachable.

## Signing-key compromise

Treat compromise as affecting every audience signed by that key.

1. Restrict ingress and internal network reachability; disable affected routes
   if arbitrary signed requests may be reaching consumers.
2. Generate a new independent key, distribute its public JWK, and deploy the new
   signer through the fastest safe coordinated path.
3. Remove the compromised public JWK as soon as the new signer is active. An
   emergency rotation may intentionally skip the normal overlap wait because
   continuing to accept the compromised key is worse than rejecting in-flight
   tokens.
4. Restart every Stargate replica to remove the compromised in-memory signer.
5. Review bounded issuance/verification telemetry, access records, audit
   events, and secret-system access without collecting compact tokens.
6. Rotate any unrelated credential exposed by the same incident and document
   the affected time window, audiences, services, and response.

Deleting only the private-key file does not remove a key already loaded into a
running process.

## Signing admission

Internal-context dispatch uses a fixed worker pool, initialized on the first
signed attempt. Worker and queue settings take effect at process startup. RSA
operations run outside Tokio. The queue has a finite capacity and a deadline to
start work; full or expired queues return `503 gateway.overloaded`
with `phase=signing` and a bounded reason, before any network attempt. This local
failure does not trigger failover or change upstream health. Failed worker startup
returns the existing preparation error. Monitor the issuance failure reasons and
`stargate.gateway.rejections{kind=signing}` alongside queue wait.

Cancellation drops the reply receivers, so waiting work is skipped. An RSA
operation that has already started finishes on its worker; at most the configured
worker count can be running, and its result is discarded if the request ended.
Queued jobs retain owned issuance facts, rather than request bodies or admission
permits. Workers stop when the runtime's sender is dropped; the process runtime
normally lives until exit. Every accepted attempt still mints its own token,
including fresh `jti`, after final request sanitization and binding. No tokens are
cached or shared between attempts.

These defaults protect unrelated I/O but do not establish production capacity.
Increase workers only with spare CPU and measured tail-latency headroom; increasing
queue capacity cannot increase signing throughput. See the release gateway
[performance measurements](gateway-performance.md) for the mixed-traffic evidence,
resource budgets, and repeatable commands.

## Signing benchmark baseline

The benchmark uses the release profile, a representative user + organization
context, a 2,048-bit RSA key, and the same `ContextSigner` used by Stargate.
Run it with:

```bash
cargo bench -p ctx --bench internal_context_signing -- --quick
```

Baseline captured 2026-07-21 on the local Darwin development environment with
Rust 1.96.0. Quick sampling is suitable for a regression reference, not
production capacity planning.

| Case | Median batch time | Throughput |
| --- | ---: | ---: |
| Single signer | 1.451 ms per token | 689 tokens/s |
| 2-worker pool, batches of 32 | 25.405 ms | 1,260 tokens/s |
| 4-worker pool, batches of 64 | 25.717 ms | 2,489 tokens/s |

The two-worker and four-worker cases reached about 1.83x and 3.61x the
single-worker throughput, approximately 91% and 90% parallel efficiency. The
shell wrapper reported 7.94 seconds user + 0.42 seconds system over 13.002
seconds wall time (64% aggregate CPU), including Criterion warmup and analysis;
use the per-worker throughput above for sizing rather than the wrapper CPU
percentage.

The representative compact header value was 1,511 bytes, about 37% of the
4,096-byte limit and approximately 1,531 bytes as an HTTP/1 field line including
the `stargate-context` name, separator, and CRLF. Re-run the benchmark on target
hardware and under the target runtime concurrency before enabling a high-volume
upstream.

## Release checklist

- [ ] Every real upstream appears in the enablement inventory with an explicit
      `internal-context` or `external/pass-through` classification.
- [ ] Every enabled audience has a consumer owner and deployment evidence.
- [ ] TLS/network controls and direct-bypass rejection are verified.
- [ ] The planned-rotation and fail-closed drills pass.
- [ ] Issuance failure, configuration failure, signing latency, and token-size
      telemetry are exported and alertable.
- [ ] Logs and errors contain no compact token or decoded payload.
- [ ] UTC clock synchronization is healthy on Stargate and consumers.
- [ ] Complete edge tests, cluster checks, and strict Clippy pass.
- [ ] Any unavailable external integration infrastructure is explicitly noted
      rather than treated as successful evidence.

At present, the repository inventory contains no environment-owned upstream
graph and therefore claims no external deployment evidence.
