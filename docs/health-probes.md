# Health and readiness probes

Stargate exposes process liveness separately from traffic readiness. This
follows the [Kubernetes probe model](https://kubernetes.io/docs/concepts/workloads/pods/probes/):
liveness failure can restart a container, while readiness failure removes it
from service traffic without restarting it.

| Endpoint | Success response | Failure response | Purpose |
| --- | --- | --- | --- |
| `GET /livez` | `200 {"status":"alive"}` | No application-level failure status; an unresponsive server fails the caller's probe | Startup and liveness |
| `GET /readyz` | `200 {"status":"ready"}` | `503 {"status":"not_ready"}` | Traffic readiness |
| `GET /admin/health` | `200` with component status, latency, backend kinds, and timestamp | `503` with component diagnostics | Authenticated operator diagnostics |

The two public endpoints also support `HEAD`, return `Cache-Control: no-store`,
and bypass authentication, API rate limiting, gateway ingress/process admission,
and gateway routing policies.
They use the normal HTTP or HTTPS listener; no extra port is required.
Dependency errors and latency details appear only in `/admin/health`, which
retains its super-admin authorization and normal API middleware.

## Readiness conditions

Readiness succeeds once initialization has finished, shutdown has not begun,
and the required dependencies respond:

- Edge: SQL `SELECT 1` against SQLite. The in-memory state store is initialized
  during startup and has no remote connectivity check.
- Cluster: SQL `SELECT 1` against PostgreSQL and `PING` against the initialized
  Redis state-store pool. Both checks run concurrently.

Each dependency check has a one-second deadline, including SQL pool acquisition.
Errors, missing initialized dependencies, and timeouts produce `503`. Checks
run on each readiness request; responses are not cached. A later successful
check restores readiness automatically after a dependency outage.
`/admin/health` uses the same bounded dependency checks. It also reports
`config: { gatewayRevision, policiesRevision }`, the exact document revisions
currently active on the responding replica. A failed activation or a pending
capacity restart can leave these behind the latest database revisions. Notification
and activation failures do not add readiness conditions; the dependency checks remain
unchanged. Cluster synchronization checks after PostgreSQL notifications/reconnections
and at `CONFIG_SYNC_INTERVAL_SECS` (default 30 seconds) to recover missed notifications.

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

The cluster configuration synchronizer is stopped and awaited before the audit relay.

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
The default three-second probe timeout exceeds the one-second dependency
deadline. Probe intervals and failure thresholds determine how quickly
Kubernetes reacts to failures.
