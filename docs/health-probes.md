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
They use the normal HTTP or HTTPS listener; no extra port is required.
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
also includes `limits: { storeDegraded }`, true after any traffic limit store
error until that application point succeeds again. The overall status is
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

With Redis unavailable after startup, replicas remain ready while SQL is healthy.
Traffic limits default to fail-open: public gateway routes, direct responses,
static auth/Console pages, discovery/JWKS and API/admin keys verified through SQL
remain usable. The affected checks emit no allowance headers. Each check is
bounded by `ingress.timeout` (default 250ms), including rate/quota policies and
admin principal admission. Set `on_store_error: deny` on ingress or a route's
rate/quota policy to retain `503` with `Retry-After: 10` for that check.
Configuration errors stay `500`; login guard, OTP/email throttles, sessions,
JWT revocation and OAuth state remain fail-closed. User login and the Console's
user session therefore remain unavailable. Redis recovery restores checks
without a restart. A Traefik rate limit provides a safety net while the shared
per-IP allowance is unavailable.

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
