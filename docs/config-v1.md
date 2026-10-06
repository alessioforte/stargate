# Stargate Config v1

Stargate now expects `config.yaml` to use the explicit v1 schema:

```yaml
schema: stargate/v1
```

Every supplied YAML document and admin JSON submission must explicitly contain
`schema: stargate/v1`. Missing and unsupported identifiers are rejected; there is
no compatibility parser or automatic conversion. Generated defaults use this
identifier too. Startup, hot reload, and admin writes compile the candidate and
prepare its HTTP/TLS transports and internal-context signer requirements first.
Invalid reloads leave the previous routes and transports active. Invalid admin
submissions are not saved, and invalid initial preparation stops startup before
readiness.

This is the first gateway configuration format, not an application release
version. Signed internal-context and other independently versioned contracts
retain their own version identifiers.

Gateway configuration activates as one runtime snapshot containing the compiled
graph, balancers, limiter, prepared HTTP clients, and a configuration version.
Each request pins it before authentication and uses it for routing, policy
checks, planning, every primary/failover attempt, and all mirrors. Reloads apply
to new requests; existing requests, streaming response bodies, and WebSocket
tasks retain their generation. A successful activation starts the new health
probes and retires the old probe tasks. Failed preparation changes neither the
active snapshot/version nor its probes.

IAM/admin rate-limit middleware also pins a snapshot for downstream handlers.
Admin overview reports its graph counts and `configVersion` together. The
independent ACE policy engine/revision pair is loaded once per request; policy
file updates change that revision without changing `configVersion`.

GCRA limits require a nonzero `max_burst` and a positive, representable
`replenish_1_per`. The combined duration/burst must fit the supported timestamp
range. Liveness probe intervals default to `5s`; explicitly configured zero,
malformed, or unrepresentable intervals are rejected before activation. These
errors also reject admin saves.

## Model

The gateway config is split into named objects:

- `ingress`: mandatory pre-authentication rate-limit binding and store deadline.
- `limits`: named rate-limit and quota strategies referenced by ingress and policies.
- `upstreams`: physical target pools.
- `services`: traffic actions that point to upstreams or compose other services.
- `middlewares`: request/response transforms.
- `policies`: auth, access-control, rate-limit, and quota checks.
- `routers`: ordered match rules that bind traffic to services, middlewares, and policies.

Routers are sorted by descending `priority`. Ties keep declaration order. Use high positive priorities for specific rules and a low negative priority for fallback routes.

## Minimal Example

```yaml
schema: stargate/v1

ingress:
  limit: ingress
  timeout: 250ms

limits:
  ingress:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 100ms
  default:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 1s

http:
  upstreams:
    users:
      targets:
        - url: http://users:8080
      load_balancer:
        strategy: round_robin
      transport:
        connect_timeout: 2s

  services:
    users:
      kind: load_balancer
      upstream: users

  policies:
    auth:
      kind: auth
      strategies: [jwt, api_key]
    default-rate:
      kind: rate_limit
      limit: default

  routers:
    users:
      priority: 100
      match:
        path:
          prefix: /api/users
      service: users
      policies: [auth, default-rate]
```

## Ingress admission

Every supplied YAML config and admin JSON submission must contain `ingress.limit`
referencing an existing named `gcra` or `token_bucket` rate limit. There is no
implicit binding or disable switch. Quota trackers, missing/blank references,
zero token-bucket capacity/refill rate, overflowing bucket lifetimes, and invalid durations reject startup,
reload, and admin saves. `ingress.timeout` defaults to `250ms` and must be a
positive, representable duration. Generated configs and the Helm defaults use
burst 100 with one request replenished every `100ms` (10/s); tune these values
for the traffic sharing each client IP.

Gateway requests first acquire process admission, then resolve the client IP and
consume its ingress allowance, before credential verification, database lookup,
or route matching. Invalid API keys/bearer tokens, rejected auth/access policies,
and nonexistent routes all consume allowance. Keys use the resolved IP only;
changing credentials or client headers cannot choose another bucket. Existing
`TRUSTED_PROXIES` rules accept `X-Forwarded-For`/`X-Real-IP` only from trusted
peers; an untrusted peer is keyed by its socket address. Without a peer address,
requests share the conservative `unknown` bucket. Keep trusted-proxy CIDRs
restricted to the deployment's actual proxy peers.

Ingress uses its own key namespace, distinct from resource subject/org rate and
quota buckets even when both reference the same named limit. After admission,
authentication and selected resource policies still enforce their own limits
and `quota_cost`. Ingress allowances persist through reloads of the same named
limit because generations share limiter state; each request pins its ingress
binding and deadline with its runtime snapshot.

Ingress denial returns `429 gateway.ingress_rate_limit_exceeded` with integer
seconds in `Retry-After` and `X-RateLimit-Scope: ingress`. Missing runtime limits,
backend failures, or an ingress-store timeout fail closed with
`503 gateway.ingress_unavailable`; the enclosing total request deadline still
returns `504 gateway.timeout` if it expires first. Process exhaustion retains
`503 gateway.overloaded`. Allowed ingress checks do not overwrite resource
response headers.

`/livez`, `/readyz`, and `/health` bypass gateway admission and IAM rate limiting.
Explicit IAM/admin endpoints retain their existing `default` rate-limit
middleware; gateway ingress applies only to the gateway fallback. The console's
gateway page exposes the mandatory ingress limit and store timeout. Editing
named objects preserves this top-level binding; when renaming/removing its
selected limit, select an existing replacement before saving.

## HTTP and WebSocket transport

`http.upstreams.<name>.transport.connect_timeout` defaults to `runtime.connect_timeout` (`5s`). A supplied
value must be a positive, representable integer duration with one of the units
`ns`, `us`, `ms`, `s`, `m`, `h`, `d`, or `w`. Zero, malformed, negative, and
overflowing values are rejected, including on upstreams that no service uses.
This setting bounds DNS, TCP, and TLS establishment for both HTTP and WebSocket
connections; response headers and lifecycle deadlines are separate concerns.

`transport.protocols` controls the prepared HTTP client's allowed protocols.
Omitting it or supplying `[]` permits HTTP/1.1 and HTTP/2. `[http1]` uses HTTP/1;
`[http2]` uses HTTP/2, including prior knowledge for cleartext targets. The
downstream request version does not override this upstream policy. WebSocket
proxying uses the HTTP/1.1 Upgrade handshake from
[RFC 6455 section 4](https://www.rfc-editor.org/rfc/rfc6455.html#section-4), so its
upstream must permit `http1`. An HTTP/2-only upstream rejects WebSocket dispatch
with `500 gateway.request_preparation_failed` and reason
`websocket_requires_http1`, before connecting. HTTP/2 extended CONNECT is not
supported. Downstream upgrades require a valid HTTP/1.1 `GET`, a single Host,
version `13`, and a single base64 key decoding to 16 bytes; invalid handshakes
return `400 gateway.websocket_upgrade_invalid` before upstream dispatch.

HTTPS uses the bundled WebPKI roots by default. The optional top-level `mtls`
block supplies the same custom server trust store and client identity to HTTPS
and secure WebSocket upstreams:

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
mtls:
  ca_cert_path: /etc/stargate/upstream-ca.pem
  client_cert_path: /etc/stargate/client-chain.pem
  client_key_path: /etc/stargate/client-key.pem
```

Both certificate files must contain at least one valid PEM-encoded X.509
certificate. Put the client leaf certificate first, followed by any
intermediates. The key file must contain exactly one supported, unencrypted
PKCS#1, PKCS#8, or SEC1 private key matching the leaf certificate. Missing,
empty, malformed, and mismatched material rejects the whole candidate; there is
no fallback to another identity. A supplied `mtls` block is checked even when
the candidate has no HTTPS upstreams. Server identity, trust, and validity are
still verified during each TLS handshake.

Clients are built before activation. Request-time lookup only clones a prepared
client and performs no credential-file reads or TLS construction. Replacing
credential files alone does not reload the clients: change the gateway
configuration content to trigger preparation. If a candidate fails, fix its
files and save that candidate again; existing prepared clients remain usable
throughout.

WebSockets use the pinned transport's prepared trust roots and client identity,
with ALPN restricted to `http/1.1`. The connection's DNS destination, TLS server
name, and certificate verification always use the upstream URI. The
`preserve_host` middleware replaces the generated Host with exactly one incoming
value; it does not change the TLS identity or destination. Without it, the
handshake uses the target Host. HTTP and WebSocket requests share hop-by-hop
sanitization, including fields named by every `Connection` value. The WebSocket
client regenerates its Upgrade, key, and version fields after that cleanup.
Origin and offered subprotocols are forwarded unless explicitly nominated as
hop-by-hop fields; multiple subprotocol fields are combined for negotiation.
The upstream's selected subprotocol is returned to the downstream client.
Extensions are not offered, and an upstream response that selects an extension
or repeats the selected subprotocol is rejected. Internal-context sanitization
and per-attempt signing run after the final handshake URI and headers are set.

## Routing

Supported matchers:

```yaml
match:
  all:
    - method: [GET, POST]
    - host:
        eq: api.example.com
    - path:
        prefix: /api/reports
    - header:
        name: x-version
        one_of: [v2, canary]
    - query:
        name: preview
        eq: "true"
    - cookie:
        name: cohort
        regex: "^(beta|staff)$"
    - source_ip:
        cidrs: [10.0.0.0/8, 127.0.0.1/32]
```

Matcher combinators:

```yaml
match:
  any:
    - path: { exact: /health }
    - path: { exact: /ready }
```

```yaml
match:
  not:
    header:
      name: x-blocked
      present: true
```

Path matcher operators:

- `exact`
- `prefix`
- `template`, for example `/api/users/{id}`
- `regex`

Value matcher operators for `host`, `header`, `query`, and `cookie`:

- `eq`
- `prefix`
- `suffix`
- `contains`
- `regex`
- `present`
- `one_of`

Exactly one operator must be set per matcher.

Fallback route:

```yaml
http:
  services:
    not-found:
      kind: direct_response
      status: 404
      body:
        json:
          code: ROUTE_NOT_FOUND

  routers:
    fallback:
      priority: -1000
      match:
        path:
          prefix: /
      service: not-found
```

## Middleware

Path rewrite:

```yaml
http:
  middlewares:
    strip-api:
      kind: strip_prefix
      prefixes: [/api]

    add-v2:
      kind: add_prefix
      prefix: /v2

    legacy-rewrite:
      kind: replace_path_regex
      pattern: ^/api/legacy/(.*)$
      replacement: /v2/$1
```

Header transforms:

```yaml
http:
  middlewares:
    request-headers:
      kind: request_headers
      remove: [x-debug]
      set:
        - name: x-forwarded-app
          value: stargate
      add:
        - name: x-extra
          value: "1"

    response-headers:
      kind: response_headers
      remove: [server]
      set:
        - name: cache-control
          value: no-store
```

Preserve original `Host` when proxying:

```yaml
http:
  middlewares:
    preserve-host:
      kind: preserve_host
```

## Traffic Services

Load balanced upstream:

```yaml
http:
  upstreams:
    api-v1:
      targets:
        - url: http://api-v1-a:8080
        - url: http://api-v1-b:8080
      load_balancer:
        strategy: round_robin

  services:
    api-v1:
      kind: load_balancer
      upstream: api-v1
```

### Internal context

An upstream may declare the audience that will receive Stargate's signed
internal request context:

```yaml
http:
  upstreams:
    orders:
      targets:
        - url: http://orders:8080
      internal_context:
        audience: urn:stargate:service:orders
```

`audience` is required, must not be blank, and is limited to 256 UTF-8 bytes.
Omitting `internal_context` preserves the existing upstream behavior and emits
no signed context.

Stargate parses and compiles the block and preflights the separate signing key
and matching public `kid` before activation. A rejected startup or hot reload
never replaces the previous active graph. Do not enable the block for an
upstream until its deployed consumer follows the fail-closed validation and
ownership requirements in `docs/internal-context-consumer-guide.md`. Keep real
enablement evidence with the responsible deployment or release record.

`stargate-context` is reserved to the gateway. It cannot be added, set, or
removed by request/response header middleware, and cannot be returned by a
`direct_response` service. Matching is case-insensitive.

Signed HTTP attempts and WebSocket handshakes await a fixed CPU worker pool.
Its queue capacity/deadline are configured with `INTERNAL_CONTEXT_SIGNING_*`
environment variables, independently from the gateway YAML. Full/expired queues
return local `503 gateway.overloaded` (`phase=signing`) without failover or
upstream health changes. See [signing admission](internal-context-operations.md#signing-admission)
and the [performance report](gateway-performance.md) for defaults and measurements.

The signer is configured independently from OAuth/OIDC keys:

```bash
cargo run --features edge -- internal-context generate-key \
  --output-dir .stargate/internal-context \
  --kid stargate-internal-current
```

The command creates `private.pem` and a matching public-only `jwks.json`, sets
restrictive Unix permissions, and refuses to overwrite either file. RSA keys
default to 2,048 bits; `--bits` accepts values from 2,048 through 8,192.

```dotenv
INTERNAL_CONTEXT_ALGORITHM=RS256
INTERNAL_CONTEXT_ISSUER=https://auth.example.com/internal-context
INTERNAL_CONTEXT_KID=stargate-internal-current
INTERNAL_CONTEXT_PRIVATE_KEY_PATH=.stargate/internal-context/private.pem
INTERNAL_CONTEXT_JWKS_PATH=.stargate/internal-context/jwks.json
INTERNAL_CONTEXT_TTL_SECS=30
INTERNAL_CONTEXT_CLOCK_SKEW_SECS=5
INTERNAL_CONTEXT_JWKS_CACHE_MAX_AGE_SECS=60
```

Only RS256 is accepted in version 1. Server startup does not generate missing
internal keys; generation is an explicit CLI action. The JWKS file must contain
public RSA keys with unique `kid` values and may contain both current and
retiring keys. The active `kid` must match the private signing key. Public keys
are exposed separately at `/.well-known/stargate-context-jwks.json`; the
OAuth/OIDC JWKS is unchanged.

Rotate keys in this order:

1. Roll/restart Stargate with the current signer and both public JWKs.
2. Allow downstream caches to observe the overlapping JWKS and validate the
   new key through unknown-`kid` refresh.
3. Roll/restart Stargate with the new private key and
   `INTERNAL_CONTEXT_KID`, while continuing to publish both public keys.
4. After the last old signer stops, retain the old public key for at least the
   token TTL, maximum clock skew, and downstream JWKS cache overlap.
5. Remove the old public key and roll/restart again.

The signer and JWKS are loaded into memory at process startup; editing the
files alone does not rotate a running process. Metrics, alert guidance,
permissions, drills, benchmarks, rotation, recovery, and compromise response
are documented in `docs/internal-context-operations.md`.

### Upstream Health

Each upstream target is fronted by a circuit breaker. Two independent signals
drive it:

- **Active liveness probe** — periodic request to `liveness_probe.path`.
- **Passive feedback** — outcomes of live proxied traffic. Transport errors and
  `502`/`503`/`504` responses count as failures; any other received response
  counts as a success. Mirror (shadow) traffic never affects the breaker.

After `circuit_breaker.fail_threshold` consecutive failures the target is
removed from selection for `circuit_breaker.cooldown`, then a single probe
request is admitted to test recovery before traffic is restored.

```yaml
http:
  upstreams:
    api-v1:
      targets:
        - url: http://api-v1-a:8080
        - url: http://api-v1-b:8080
      load_balancer:
        strategy: round_robin
        liveness_probe:
          path: /health
          interval: 5s          # default 5s
        circuit_breaker:
          fail_threshold: 3      # default 3 consecutive failures
          cooldown: 30s          # default 30s before a recovery probe
```

Both `liveness_probe` and `circuit_breaker` are optional; omitting
`circuit_breaker` uses the defaults shown above.

Weighted traffic split:

```yaml
http:
  services:
    api-split:
      kind: weighted
      services:
        - name: api-v1
          weight: 95
        - name: api-v2
          weight: 5
```

Mirror traffic:

```yaml
http:
  services:
    api-with-shadow:
      kind: mirror
      service: api-v1
      mirrors:
        - service: shadow-api
          percent: 100
```

Failover by transport error and selected response status for eligible operations:

```yaml
http:
  services:
    api-resilient:
      kind: failover
      # Automatic replay uses the eligible methods listed below; POST/PATCH dispatch once.
      service: api-primary
      failovers: [api-backup]
      on_status: [502, 503, 504]
```

Direct response:

```yaml
http:
  services:
    maintenance:
      kind: direct_response
      status: 503
      headers:
        - name: retry-after
          value: "60"
      body:
        text: maintenance
```

Admitted mirrors and eligible upstream attempts with possible later failover
branches buffer the request body, including transport-only failover. A chain of
local direct responses does not need a failover buffer. The per-request cap is `runtime.replay_body_bytes`
(default `2097152`, or 2 MiB). `GATEWAY_REPLAY_BODY_LIMIT` has been removed.
When only mirrors need replay, an oversized `Content-Length` skips mirrors and
streams the primary request. A buffered body crossing the per-request cap
returns `413 gateway.replay_payload_too_large`.

Nested services retain their execution boundaries. A child completes its own
failover chain before its parent examines the resulting response or transport
error. Each failover service applies only its own `on_status` list: a parent's
`[503]` does not adopt a fallback child's `[404]`. Weighted choices remain
deterministic for the request seed; an unavailable chosen branch can be skipped
by its enclosing failover without selecting another weighted choice.

Upstreams are selected when execution enters their leaf, so unused fallback
branches do not advance a balancer or reserve a half-open circuit probe. If all
remaining branches are unavailable, the last response (including its body) or
transport error is returned. Direct responses and unavailable leaves do not
increment network attempt numbers; nested network dispatches share the primary
counter, and each shadow task starts its own counter at one.

Mirrors belong to the service branch that declares them. Each entered mirror
boundary dispatches its sampled, admitted shadows once, when its main branch
reaches an available upstream or direct response. Shadows of unused or entirely
unavailable branches receive no traffic and reserve no capacity. A mirror
around a failover runs once for that branch; a mirror inside a selected fallback
runs only when that fallback is entered. Shadow plans follow the same nested
rules, replay eligibility, and shared resource budgets. An unused fallback's
mirrors cannot require a mutation's body to buffer.

Discarded responses are consumed frame by frame, without collecting them, up to
`runtime.discarded_body_bytes` (default 64 KiB) and an absolute
`runtime.discarded_body_timeout` (default 250 ms). Progress cannot reset that
absolute deadline. The remaining request/mirror budget can end disposal sooner.
An already-received frame may cross the byte cap; no subsequent frame is read.
Small bodies can drain fully and allow connection reuse.

## Automatic replay and mutations

Automatic failover is eligible only for `GET`, `HEAD`, `OPTIONS`, `TRACE`, `PUT`,
and `DELETE`, following [RFC 9110 §9.2.2](https://www.rfc-editor.org/rfc/rfc9110.html#section-9.2.2).
The same eligibility decision governs transport-error and configured
`on_status` failover, including shadow plans. `POST`, `PATCH`, `CONNECT`, and
unrecognized methods receive one dispatch attempt: after an attempted dispatch,
a connection failure returns the error and a configured failure status returns
that response with its body intact. The backend might have committed before the
connection failed, so an absent response is insufficient evidence to retry.

An `Idempotency-Key` header does not establish idempotent semantics. There is no
route override or mutation-retry switch. A future extension for idempotent
operations on other methods would require an explicit operation contract and
integration proof of deduplication shared by every possible target. Clients
should reconcile uncertain mutation outcomes using their application's own
operation contract before deciding whether to submit another request.

Unavailable/unhealthy candidates may still be skipped during service selection,
before any dispatch. Thus a mutation can go directly to an available fallback
without repeating an attempted operation. Signing/preparation failures and
client-body errors terminate the request instead of triggering failover or
penalizing upstream health. The prepared HTTP clients disable hidden transport
retries; the executor owns every automatic retry and issues fresh context per
eligible network attempt, with the selected audience and attempt number.

Eligibility is determined before reserving replay storage or reading the body.
A mutation without admitted mirrors streams beyond `runtime.replay_body_bytes`,
even if its service has alternatives or the replay-memory budget is occupied.
Eligible multi-attempt plans retain their required replay cap and reservation.
Mirrors are deliberate shadow copies and still require their existing buffer
and budgets; buffering a mutation for a mirror does not make its primary or
shadow failover eligible. For optional mirrors, known oversized bodies or
reservation failures skip the mirrors as before; an unknown-length body that
exceeds an admitted mirror buffer is rejected before any dispatch.

## Runtime deadlines and resource budgets

All settings below are optional. Durations must be positive and representable;
capacities and byte counts must be positive bounded integers. Unknown `runtime`
fields are rejected. `replay_body_bytes` cannot exceed `replay_memory_bytes`.
These defaults are a finite baseline for the later capacity measurements.

```yaml
runtime:
  primary_concurrency: 1024
  mirror_concurrency: 64
  replay_body_bytes: 2097152
  replay_memory_bytes: 67108864
  upload_idle_timeout: 15s
  connect_timeout: 5s
  response_header_timeout: 30s
  response_body_idle_timeout: 30s
  discarded_body_timeout: 250ms
  discarded_body_bytes: 65536
  mirror_timeout: 5s
  request_timeout: 60s
```

Primary admission uses a process-wide permit before authentication and route
matching. Exhaustion returns `503 gateway.overloaded` promptly. A permit stays
with the response body through completion, failure, or drop, and with an
upgraded WebSocket task until the session ends. Mirror permits are acquired
before buffering/spawning; excess candidates are skipped without waiting tasks.
If mirrors are the only reason to buffer, exhausted mirror or memory capacity
skips mirroring and streams the primary. If failover requires buffering,
exhausted memory returns `503 gateway.replay_memory_exhausted`.

Each replay buffer reserves its full per-request cap before allocation, keeping
allocated replay capacity within the aggregate budget. Immutable bytes share
that reservation across primary attempts, mirror tasks, and rebuilt outbound
bodies. It is released when the last byte owner drops. This budget covers replay
storage, not Hyper/TLS/network buffers. With the defaults, up to 32 full replay
reservations fit within 64 MiB; the 64 mirror slots also cover other phases.

`primary_concurrency`, `mirror_concurrency`, and `replay_memory_bytes` are fixed
at startup. Reloads share the same process controller; candidates changing these
capacities are rejected and leave the previous snapshot active. Restart to apply
capacity changes. Other settings are validated, published, and pinned with the
request's snapshot.

A finite request has one absolute `request_timeout` beginning at admission,
covering authentication, policy work, upload, all failover attempts, disposal,
and response body forwarding. Each attempt also has a response-header deadline
from dispatch, including connection and upload. The HTTP connection deadline
covers DNS, TCP, and TLS together, using the upstream override when supplied.
An upload with no nonempty data or trailers for `upload_idle_timeout` returns
`408 gateway.upload_timeout`. The same idle policy applies to replay buffering
and directly streamed uploads; empty frames cannot keep an upload alive.

Long-lived HTTP responses require an explicit route policy:

```yaml
http:
  routers:
    events:
      match: { path: { prefix: /events } }
      service: event-service
      response_mode: stream  # default: finite
```

`stream` retains the finite budget through response headers, then uses
`response_body_idle_timeout` and primary concurrency admission without a fixed
response lifetime. Nonempty data and trailers count as progress. WebSocket
sessions always follow the stream policy. HTTP uploads remain finite and
use the upload idle and request budgets, including on response-stream routes.
WebSocket DNS/TCP/TLS uses the selected upstream's connect timeout; the upstream
Upgrade handshake then uses `response_header_timeout`. Both are capped by the
remaining total budget, as is the downstream upgrade. Frame receipt and
forwarding use the body idle policy; a stalled sink also expires. The tracked
task retains its runtime and primary admission until disconnect, idle expiry,
or shutdown. WebSockets use the first selected upstream and do not replay or
mirror frames.

Before headers, deadline expiry returns `504 gateway.timeout` with a bounded
`phase` parameter, and shutdown cancellation returns `503 gateway.cancelled`.
After headers, expiry/cancellation terminates the body/session; it cannot replace
the committed status with JSON or start a retry. Local timeout/cancellation does
not eject a target from the circuit breaker. Transport failure remains
`502 upstream.connection_failed`; automatic failover requires an eligible
operation and an available alternative.

Mirror tasks have an independent absolute `mirror_timeout`, including every
attempt and disposal; they may finish after the primary. Both mirrors and
WebSocket tasks are tracked. After the shutdown readiness delay, the shared
controller rejects new admission and cancels active gateway work. HTTP and TLS
listeners stop, connections drain within `SERVER_SHUTDOWN_TIMEOUT_SECS`, remaining
connections are cancelled, and tracked gateway tasks are drained before hooks.

`stargate.gateway.resources.active` is an up/down counter with only `kind`:
`primary` (requests/sessions), `mirror` (reserved slots/tasks), and `replay_memory`
(reserved bytes). All three return to zero after gateway work drains.
`stargate.gateway.rejections` uses bounded `kind` labels: `primary`,
`replay_memory`, `ingress`, and `signing`.
`stargate.gateway.timeouts` uses bounded phase labels (`total`, `upload`, `ingress`,
`connect`, `response_headers`, `response_body`, `disposal`,
`websocket_handshake`, `websocket_idle`). None contain URLs, credentials, or
request IDs.

`stargate.gateway.duration` and `stargate.gateway.upstream.duration` end at
response headers. The upstream instrument includes dispatch preparation and
signing. `stargate.gateway.transfer.duration` records milliseconds until each
upload, downstream body, or accepted WebSocket completes or terminates. Its only
labels are `phase` (`upload`, `response_body`, `websocket`) and
`stargate.outcome` (`complete`, `error`, `timeout`, `cancelled`, `dropped`). Each
guarded downstream body is recorded once, including empty bodies; a dropped client body
and process cancellation are separate outcomes. Body lifetime starts when the
response wrapper is created, so it is a separate phase, not the end-to-end
duration. WebSocket lifetime starts after the upstream handshake.
Local errors returned before a response wrapper is created have header latency only.

`stargate.config.reloads` uses `stargate.config_kind` (`gateway_config`,
`policies`) and bounded `stargate.outcome` (`success`, `error`,
`transport_error`, `internal_context_error`). Preparation failures retain the
current snapshot; repaired TLS files can activate on a later reload. No metric
label contains a file path, target URL, token, or request ID.

`stargate.gateway.response.disposals` counts completed disposal outcomes;
`stargate.gateway.response.disposal.bytes` and
`stargate.gateway.response.disposal.duration` record observed data bytes and
elapsed milliseconds. Their only dimensions are `stargate.disposal.kind`
(`failover`, `mirror`) and `stargate.outcome` (`drained`, `byte_limit`, `timeout`,
`body_error`, `cancelled`). They contain no target URLs, credentials, or body contents.
`stargate.gateway.mirrors` records `dispatched` at task creation and a final
`success`, `abandoned`, `timeout`, `body_error`, `cancelled`, or `error` after execution and
disposal finish. Skipped candidates use `skipped_capacity`, `skipped_memory`,
`skipped_payload`, or `skipped_shutdown`. `abandoned` means the byte budget was reached; `error` means
execution failed before a disposable response was available. Shadow outcomes
never change primary upstream health.

## Policies

```yaml
http:
  policies:
    auth:
      kind: auth
      strategies: [jwt, api_key]

    app-user-auth:
      kind: auth
      strategies: [oauth]
      audience: gateway

    reports-read:
      kind: access_control
      resource: financial_reports:read

    burst:
      kind: rate_limit
      limit: default

    daily-quota:
      kind: quota
      limit: daily
```

`jwt` accepts Stargate native session tokens, `api_key` accepts API keys, and
`oauth` accepts user access tokens issued by Stargate's Authorization Code
flow. Every policy containing `oauth` must configure one exact `audience`.
The OAuth client must be registered for that audience and request it during
authorization. OAuth client-credentials tokens are not user identities and
are not accepted by this strategy.

Attach policies on a router:

```yaml
http:
  routers:
    reports:
      priority: 100
      match:
        path:
          prefix: /api/reports
      service: reports
      policies: [auth, reports-read, burst, daily-quota]
```

Policy order in the runtime is fixed: auth, access control, rate limit, quota. The list on the router declares which policies apply.

Gateway requests always run through rate-limit selection. If no router policy
selects a limit, the gateway uses the named `default` limit. A router
`rate_limit` policy is the resource-specific override:

```yaml
http:
  policies:
    reports-burst:
      kind: rate_limit
      limit: reports
```

Every supplied config must include `limits.default`, using GCRA or a positive,
representable token bucket. The implicit resource check and IAM/admin middleware
select this default by name; deleting it is a compile error. Generated configs
provide GCRA burst `100` replenishing one request per `1s`, alongside the separate
mandatory ingress limit. There is no allow-on-missing or resource rate-limit
disable switch. Rate policies must reference rate strategies, and quota policies
must reference `quota_tracker`.

Authenticated users and API keys can also select a rate limit by setting
`attrs.rate_limit` to a named limit. This overrides the implicit default, and
also overrides an explicit router policy whose limit is `default`. A non-default
router policy still wins, so endpoint-specific limits stay enforced.

Quota is selected independently from rate limiting. A router `quota` policy or
subject `attrs.quota` selects a named `quota_tracker` limit; when both a rate
limit and quota are selected, both checks run.

Ingress, resource rate/quota, and IAM/admin `429` responses emit `Retry-After`
as nonnegative decimal seconds, following
[RFC 9110 §10.2.3](https://www.rfc-editor.org/rfc/rfc9110.html#section-10.2.3).
Any fractional second rounds upward: `0s` → `0`, `500ms` → `1`, `1.001s` → `2`,
and `60s` → `60`. If the limiter supplies no retry duration, the delay is `60`
seconds. The response keeps its applicable `X-RateLimit-*` or `X-Quota-*` headers.
`503` responses, including gateway overload, retain a `10`-second `Retry-After`.
Configuration durations continue to use their normal unit-bearing format.

Selected subject and organization override names are validated against the
request's pinned runtime before any resource rate or quota bucket is charged.
Unknown names, incompatible strategies, and malformed selected attributes return
`500 gateway.limit_configuration_invalid`; the request cannot substitute a more
permissive default. Diagnostics include the policy type, scope, and a limit name
truncated to 64 Unicode characters (or the invalid attribute field); names never
become metric labels. Ingress remains the earlier, independent admission gate.

Quota is intentionally absent when neither the route nor subject selects it.
Missing or `null` override attributes leave policy selection unchanged; other
attribute types are invalid. An org check with `on_missing: skip` and no org
context is explicitly skipped. These cases do not perform a failed lookup or
manufacture an allowed decision. Valid names retain their existing independent
state namespaces and selection precedence.

Admin user, invitation, API key, and organization attribute writes reject unknown
or incompatible names with `400 request.invalid` and an `attrs.*` field parameter.
Runtime validation still applies to stored/session overrides: after a reload
removes an attribute-referenced limit, new requests fail while requests already
in flight keep their valid pinned runtime. Organization cache failures retry the
DB; a DB failure stops the checks instead of discarding stored overrides and
using a potentially more permissive route default.

The per-request cost is a property of the **route**, not the policy: routers
accept a `quota_cost` (default `1`) that says how many quota units one request
on that route consumes. It charges every quota bucket that applies — the
attached `quota` policies of any scope and the subject's `attrs.quota` — so an
expensive endpoint burns more of the same shared allowance:

```yaml
http:
  policies:
    daily-quota:
      kind: quota
      limit: daily
  routers:
    process-image:
      match:
        path:
          exact: /process_image
      service: images
      policies: [daily-quota]
      quota_cost: 5
    api:
      match:
        path:
          prefix: /
      service: api
      policies: [daily-quota]   # same bucket, cost 1
```

Setting `cost` on a `quota` policy is a compile error pointing at `quota_cost`.

### Org-scoped limits

`rate_limit` and `quota` policies accept a `scope` (default `subject`). With
`scope: org` the check consumes the bucket of the subject's active
organization (`lim:org:{org_id}` / `quota:org:{org_id}`), shared by every
member and org-bound API key acting in it. A router may carry one rate limit
and one quota **per scope**, so subject and org limits enforce side by side:

```yaml
http:
  policies:
    subject-burst:
      kind: rate_limit
      limit: default
    org-burst:
      kind: rate_limit
      limit: org-default
      scope: org
      on_missing: skip
    org-monthly:
      kind: quota
      limit: org-monthly
      scope: org
  routers:
    api:
      match:
        path:
          prefix: /api
      service: api
      policies: [auth, subject-burst, org-burst, org-monthly]
```

`on_missing` (valid only with `scope: org`) controls what happens when the
subject has no organization:

- `skip` (default): the org check is skipped; subject-scoped checks still
  apply.
- `ip_fallback`: the org limit is applied keyed by client IP instead.
- `deny`: the request is rejected with `403` — the route requires an org
  context.

Organizations override the policy's limit name the same way subjects do:
`organizations.attrs.rate_limit` / `organizations.attrs.quota` name a
configured limit spec and apply to `scope: org` checks for that org (e.g. an
enterprise org sets `attrs: { rate_limit: "org-premium" }`). The gateway
caches this subset of org attrs for ~60s; admin org updates invalidate the
cache so changes take effect on the next request.

Org checks run before subject checks. With consume-on-check strategies an
earlier bucket may be charged for a request a later check rejects — accepted
imprecision.

Response headers report the check closest to exhaustion, and
`x-ratelimit-scope` / `x-quota-scope` say which scope that was
(`org`, `subject`, or `ip` for `ip_fallback` checks). Denials carry the same
headers for the check that rejected.

`kind: access_control` points at resources evaluated by the ACE rule file
(`.stargate/policies`). The gateway config decides where a check applies; the
ACE file decides which subjects may access that resource.

Example ACE rules:

```text
# @id reports-read-admin
# @description Admins can read financial reports
ALLOW user FOR "financial_reports:READ" WHEN user.role == "admin";

# @id reports-read-analyst
ALLOW user FOR "financial_reports:READ" WHEN user.role == "analyst";
```

Subjects acting in an organization additionally expose the org context of
their session or key binding:

- `user.org_id` / `user.org_role` — the active org of the user session and
  the user's membership role in it (convention: `owner` | `admin` |
  `member`).
- `api_key.org_id` — the org a user API key is bound to, or the owning
  service account's org.

These come from the validated membership, not from subject attrs (an attr
of the same name cannot shadow them), and are absent for org-less
subjects. Keep resource names global and express org scoping in
conditions:

```text
# @id reports-org-admins
ALLOW user FOR "reports:READ" WHEN user.org_role == "admin" OR user.org_role == "owner";

# @id billing-single-org
ALLOW user FOR "billing" WHEN user.org_id == "01H8XYZ..." AND user.org_role == "owner";

# @id ingest-org-bound-keys
ALLOW api_key FOR "ingest" WHEN api_key.org_id == "01H8XYZ...";
```

The admin rule APIs manage this file directly:

- `GET /admin/access-control/rules`
- `PUT /admin/access-control/rules`
- `POST /admin/access-control/rules/validate`
- `POST /admin/access-control/rules/evaluate`
- `POST /admin/access-control/capabilities/evaluate`

The capability evaluator answers which ACE capabilities a hypothetical actor
has under supplied attributes and request context. It enumerates only resource
names declared in the active in-memory snapshot compiled from the ACE policy
file; it does not inspect or intersect gateway routers, services, or
access-control policy references. The console exposes it under **Access Control
Policies → Capabilities**.

```json
{
  "subject": "user",
  "attrs": {
    "role": "analyst",
    "department": "finance"
  },
  "orgId": "01J...",
  "orgRole": "member",
  "env": {
    "countryCode": "IT",
    "ipAddress": "203.0.113.10",
    "date": "2026-09-20",
    "time": "14:30:00"
  }
}
```

Top-level attributes are exposed under the subject namespace (`user.role` or
`api_key.role`). Organization fields are applied after attributes, so an
attribute cannot spoof `user.org_id`, `user.org_role`, or `api_key.org_id`.
Environment fields use the same `env.*` names and date/time normalization as
gateway evaluation.

The response groups allowed concrete actions by resource and separately reports
whether action-less evaluation is allowed:

```json
{
  "revision": "sha256:...",
  "capabilities": [
    {
      "resource": "financial_reports",
      "unscopedAllowed": false,
      "actions": ["READ"]
    }
  ]
}
```

Resources with no allowed action and no action-less grant are omitted. Results
use ACE's normal default-deny and explicit-deny precedence. The revision hashes
the exact policy document used for the evaluation. This endpoint is a policy
simulation from caller-supplied attributes; it does not load or verify a
persisted user or API key.

Both evaluation endpoints use the same immutable in-memory policy snapshot as
gateway authorization. They do not read or parse the policy file per request.
The snapshot atomically pairs the parsed engine with its revision, so an
evaluation and the revision returned or propagated for it cannot come from
different reloads. File changes are strictly parsed before activation; an
invalid document leaves the previous snapshot active.

`PUT` requires the current `sha256:*` revision returned by `GET`, validates the
whole document strictly, writes the file atomically, and reloads the live ACE
engine immediately. In clustered deployments this is safe only when all nodes
share the same policy file storage; otherwise the endpoint is node-local.

## Full Routing Example

```yaml
schema: stargate/v1

ingress:
  limit: ingress
  timeout: 250ms

limits:
  ingress:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 100ms
  default:
    strategy: gcra
    params:
      max_burst: 1000
      replenish_1_per: 100ms

http:
  upstreams:
    reports-v1:
      targets:
        - url: http://reports-v1:8080
    reports-v2:
      targets:
        - url: http://reports-v2:8080
    staging:
      targets:
        - url: http://reports-staging:8080
    shadow:
      targets:
        - url: http://reports-shadow:8080

  services:
    reports-v1:
      kind: load_balancer
      upstream: reports-v1
    reports-v2:
      kind: load_balancer
      upstream: reports-v2
    staging:
      kind: load_balancer
      upstream: staging
    shadow:
      kind: load_balancer
      upstream: shadow
    reports-v2-resilient:
      kind: failover
      # Status and transport retries require an eligible idempotent method.
      service: reports-v2
      failovers: [reports-v1]
      on_status: [503]
    reports-v2-mirrored:
      kind: mirror
      service: reports-v2-resilient
      mirrors:
        - service: shadow
          percent: 25
    not-found:
      kind: direct_response
      status: 404
      body:
        json:
          code: ROUTE_NOT_FOUND

  middlewares:
    strip-api:
      kind: strip_prefix
      prefixes: [/api]
    legacy-rewrite:
      kind: replace_path_regex
      pattern: ^/api/legacy/(.*)$
      replacement: /v2/$1

  policies:
    auth:
      kind: auth
      strategies: [jwt, api_key]
    default-rate:
      kind: rate_limit
      limit: default

  routers:
    reports-v2-by-header:
      priority: 1000
      match:
        all:
          - path:
              prefix: /api/reports
          - header:
              name: x-version
              eq: v2
      service: reports-v2-mirrored
      middlewares: [strip-api]
      policies: [auth, default-rate]

    reports-preview:
      priority: 900
      match:
        all:
          - path:
              prefix: /api/reports
          - query:
              name: preview
              eq: "true"
      service: staging
      middlewares: [strip-api]
      policies: [auth]

    legacy-reports:
      priority: 800
      match:
        path:
          regex: ^/api/legacy/.+$
      service: reports-v2
      middlewares: [legacy-rewrite]

    fallback:
      priority: -1000
      match:
        path:
          prefix: /
      service: not-found
```
