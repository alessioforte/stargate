# Health and readiness probes

Stargate exposes process liveness separately from traffic readiness. This
follows the [Kubernetes probe model](https://kubernetes.io/docs/concepts/workloads/pods/probes/):
liveness failure can restart a container, while readiness failure removes it
from service traffic without restarting it.

| Endpoint | Success response | Failure response | Purpose |
| --- | --- | --- | --- |
| `GET /livez` | `200 {"status":"alive"}` | No application-level failure status; an unresponsive server fails the caller's probe | Startup and liveness |
| `GET /readyz` | `200 {"status":"ready"}` | `503 {"status":"not_ready"}` | Traffic readiness |
| `GET /admin/health` | `200` with component status, latency, backend kinds, and timestamp | `503` when SQL is unhealthy, with component diagnostics | Authenticated operator diagnostics |

The two public endpoints also support `HEAD`, return `Cache-Control: no-store`,
and bypass authentication, API rate limiting, gateway ingress/process admission,
and gateway routing policies.
They belong to the management plane and answer only on a listener that declares
it. On the implicit listener they stay on `0.0.0.0:$PORT`, over HTTP or HTTPS as
before. With `LISTENERS`, a listener of only the `management` plane serves them,
never terminates TLS, and keeps them off the other ports; without such a
listener no listener serves them and startup logs a warning. See
[Listeners](listeners.md#planes).
Dependency errors and latency details appear only in `/admin/health`, which
retains its admin authorization and normal API middleware. With default
`ingress.on_store_error: allow`, an admin key verified through SQL can reach
the handler during a Redis outage. User sessions still require the store.
Setting `ingress.on_store_error: deny` blocks the handler at admission with `503`.

## Readiness conditions

Readiness succeeds once initialization has finished, shutdown has not begun,
and the required dependencies respond:

- Edge: SQL `SELECT 1` against SQLite. The in-memory state store is initialized
  during startup and has no remote connectivity check.
- Cluster: SQL `SELECT 1` against PostgreSQL. Redis does not gate readiness.

The SQL check has a one-second deadline, including pool acquisition.
SQL errors, a missing initialized pool, and timeouts produce `503`. Checks
run on each readiness request; responses are not cached. A later successful
check restores readiness automatically after a SQL outage.
`/admin/health` checks SQL and, in cluster mode, Redis concurrently with a
one-second deadline per check; a failed Redis check is reported as unhealthy,
while the HTTP response stays `200` when SQL is healthy. Inspect the body
instead of treating this status as an all-dependencies-healthy signal. The body
also includes `limits: { storeDegraded, points }`. The `points` object maps
`gateway_ingress`, `iam_ingress`, `admin_principal`, `rate_limit` and `quota` to
booleans indicating a store failure within the last 60 seconds without a subsequent
successful check; `storeDegraded` is true if any point is degraded. An idle point
expires without new traffic. Expiry describes the absence of recent failures,
not a successful Redis probe; Redis dependency diagnostics remain independent.
The overall status is
`degraded` for failed limit checks with healthy dependencies, or `unhealthy`
when a dependency fails.
It also reports
`config: { gatewayRevision, policiesRevision }`, the exact document revisions
currently active on the responding replica. A failed activation or a pending
capacity restart can leave these behind the latest database revisions. Notification
and activation failures do not add readiness conditions; the dependency checks remain
unchanged. Cluster synchronization checks after PostgreSQL notifications/reconnections
and at `CONFIG_SYNC_INTERVAL_SECS` (default 30 seconds) to recover missed notifications.
Edge uses the same interval without a listener, including for CLI imports from another
process sharing the SQLite database file.

Stargate starts without waiting for Redis, and replicas remain ready while SQL
is healthy. Each Redis connection checks for hash field expiry using
`COMMAND INFO HEXPIRE`. Use [Redis 7.4 or newer](https://redis.io/docs/latest/commands/hexpire/)
or [Valkey 9.0 or newer](https://valkey.io/commands/hexpire/) and allow the
capability command in the Redis ACL.

Traffic limits default to fail-open. Public gateway routes, static auth/Console
pages, discovery/JWKS and SQL-backed admin keys remain usable. Failed-open
checks emit no allowance headers. A request's traffic checks share
`ingress.timeout` (default 250ms), including rate/quota and admin-principal
admission. A check with no budget left applies its failure policy immediately.
Set `on_store_error: deny` on ingress or a route's rate/quota policy to return
`503` with `Retry-After: 10` when that check fails. Configuration errors stay
`500`.

Session, token revocation, cached gateway API key, OAuth state, and account-flow
state (OTP/MFA challenges and step-up markers, password-reset requests, pending
signups) fail closed on store outages. They return `503 auth.state_unavailable`
(OAuth endpoints keep their error envelope) with `Retry-After: 10`, within a
Redis time budget of one second by default. Set the positive integer
`REDIS_SECURITY_TIMEOUT_MS` at startup to change it. The preceding ingress
check counts against this budget. Once Redis becomes reachable, Stargate
reconnects without a restart. A Traefik rate limit can preserve an IP allowance
during a shared store failure.

Initialization includes database migrations, state-store setup, signing
configuration, and the compiled gateway configuration. The listener opens
after initialization, so `/livez` also works as a startup probe. Fatal startup
errors still prevent the server from listening.

Missing, unreadable, or invalid configured TLS roots/client identity reject
startup before listening. A failed TLS hot reload keeps the last valid prepared
clients and routes; restoring valid files permits the next reload to activate.

Individual gateway upstreams, SMTP, external identity providers, telemetry,
and audit relay delivery lag do not gate pod readiness. Invalid hot reloads
retain the last valid configuration and keep the current readiness behavior.
Process admission exhaustion and a full/expired internal-context signing queue
return request-level `503` responses; they do not change probe readiness.

## Failure bench

`benches/scripts/redis_failures.py` reproduces these failures against a
disposable cluster-profile deployment and asserts the behavior above. It
injects a Redis restart, an empty restart, a network partition, memory
exhaustion with `noeviction`, an unavailable or incompatible instance at startup,
and promotion of a replica after confirmed logout. It reports unexpected
responses, disturbance duration and recovery by endpoint. See
[the Redis failure bench](redis-failure-bench.md) for the scenarios, the
commands and the recorded results.

## Shutdown

The configuration synchronizer is stopped and awaited before the audit relay (when present).

On SIGTERM or SIGINT, Stargate marks readiness false immediately. It keeps
the listener open for `SERVER_DRAIN_DELAY_SECS` (default `5`, `0` disables the
delay), allowing traffic already in transit to finish reaching the pod while
load balancers learn that it is unavailable. During this window `/livez`
returns `200` and `/readyz` returns `503` without dependency I/O.
An in-flight readiness check rechecks the lifecycle before returning success.

After the delay, the server closes listeners and drains active connections,
and cancels active gateway requests, mirrors, and WebSockets. HTTP and TLS
connections share the `SERVER_SHUTDOWN_TIMEOUT_SECS` deadline (default `25`);
remaining connection tasks are cancelled when it expires. Tracked gateway tasks
are drained before the audit relay stops and the edge memory store is persisted.
The deployment termination grace period remains the final process deadline.
Queued signing jobs whose callers cancel are skipped. Already-started RSA work
cannot be preempted; the fixed worker count bounds that work until it completes.

Allow room in the pod's `terminationGracePeriodSeconds` for the delay,
connection draining, and persistence hooks. The Helm chart defaults to 40
seconds. Tune the delay to your load balancer's propagation behavior; it is
not a guarantee that every load balancer has stopped routing traffic.

## Helm

The chart uses `/livez` for startup/liveness and `/readyz` for readiness and
the Helm connection test. Custom `probes.*` overrides are supported.
The default three-second probe timeout exceeds the one-second SQL
deadline. Probe intervals and failure thresholds determine how quickly
Kubernetes reacts to failures.

## Audit diagnostics

Cluster `/admin/health` includes a cached `audit` object with `relay`
(`running`, `paused`, `failing`, `disabled`), optional `pauseReason`,
`pendingEvents`, `oldestPendingAgeSecs`, optional `streamLength` and
`oldestUnconfirmedAgeSecs`, and RFC 3339 `updatedAt`. Unknown optional
measurements are omitted. No audit queries run in the health handler.
A paused or failing audit relay does not change overall health or readiness;
alert on backlog and unconfirmed age separately. Edge omits this section.
See [Audit event delivery](audit-events.md) for consumer setup and metrics.
