# Stargate Config v2alpha1

Stargate now expects `config.yaml` to use the explicit v2 schema:

```yaml
schema: stargate/v2alpha1
```

Legacy config without `schema: stargate/v2alpha1` is rejected at load time. Hot reload compiles the new file first; invalid config is logged and the previous in-memory graph stays active.

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
schema: stargate/v2alpha1

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

## Policies

```yaml
http:
  policies:
    auth:
      kind: auth
      strategies: [jwt, api_key]

    reports-read:
      kind: access_control
      resource: financial_reports:read

    burst:
      kind: rate_limit
      limit: default

    daily-quota:
      kind: quota
      limit: daily
      cost: 1
```

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

## Full Routing Example

```yaml
schema: stargate/v2alpha1

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
