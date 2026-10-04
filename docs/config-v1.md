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

## Model

The gateway config is split into named objects:

- `limits`: named rate-limit and quota strategies referenced by policies.
- `upstreams`: physical target pools.
- `services`: traffic actions that point to upstreams or compose other services.
- `middlewares`: request/response transforms.
- `policies`: auth, access-control, rate-limit, and quota checks.
- `routers`: ordered match rules that bind traffic to services, middlewares, and policies.

Routers are sorted by descending `priority`. Ties keep declaration order. Use high positive priorities for specific rules and a low negative priority for fallback routes.

## Minimal Example

```yaml
schema: stargate/v1

limits:
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

## HTTP transport and client TLS

`http.upstreams.<name>.transport.connect_timeout` defaults to `30s`. A supplied
value must be a positive, representable integer duration with one of the units
`ns`, `us`, `ms`, `s`, `m`, `h`, `d`, or `w`. Zero, malformed, negative, and
overflowing values are rejected, including on upstreams that no service uses.
This setting bounds connection establishment; request and response lifecycle
deadlines are separate concerns.

HTTPS uses the bundled WebPKI roots by default. The optional top-level `mtls`
block supplies a custom server trust store and a client identity for HTTPS
upstreams:

```yaml
schema: stargate/v1
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

Failover by transport error and selected response status:

```yaml
http:
  services:
    api-resilient:
      kind: failover
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

Mirror and response-status failover replay the request body. Replay buffering is capped by `GATEWAY_REPLAY_BODY_LIMIT`, default `2MiB`. Accepted units: bytes, `KB`/`KiB`, `MB`/`MiB`, and `GB`/`GiB`. If only mirror traffic needs replay and `Content-Length` exceeds the cap, Stargate skips the mirror and forwards the primary request without buffering. If response-status failover needs replay and the body exceeds the cap, Stargate returns `413`.

WebSocket proxying uses the first selected upstream. Mirror traffic and response-status failover do not replay upgraded WebSocket streams.

Discarded HTTP responses (status failover and completed mirror requests) are
drained frame by frame with a fixed **64 KiB** data-byte budget and a **250 ms
absolute deadline per body**. Progress does not reset this deadline. At the
first byte limit, timeout, or body error, Stargate drops the body; failover then
advances to its next eligible attempt. Small finite bodies can finish draining
and permit connection reuse. No discarded body is collected into a buffer.
One already-received frame can cross the byte budget; its bytes are counted and
no subsequent frame is read. These fixed disposal defaults are independent of
the request replay cap. The deadline begins at body disposal, after upstream
response headers arrive.

`stargate.gateway.response.disposals` counts completed disposal outcomes;
`stargate.gateway.response.disposal.bytes` and
`stargate.gateway.response.disposal.duration` record observed data bytes and
elapsed milliseconds. Their only dimensions are `stargate.disposal.kind`
(`failover`, `mirror`) and `stargate.outcome` (`drained`, `byte_limit`, `timeout`,
`body_error`). They contain no target URLs, credentials, or body contents.
`stargate.gateway.mirrors` records `dispatched` at task creation and a final
`success`, `abandoned`, `timeout`, `body_error`, or `error` after execution and
disposal finish. `abandoned` means the byte budget was reached; `error` means
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

Authenticated users and API keys can also select a rate limit by setting
`attrs.rate_limit` to a named limit. This overrides the implicit default, and
also overrides an explicit router policy whose limit is `default`. A non-default
router policy still wins, so endpoint-specific limits stay enforced.

Quota is selected independently from rate limiting. A router `quota` policy or
subject `attrs.quota` selects a named `quota_tracker` limit; when both a rate
limit and quota are selected, both checks run.

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

limits:
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
