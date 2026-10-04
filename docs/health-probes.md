# Health and readiness probes

Stargate exposes process liveness separately from traffic readiness. This
follows the [Kubernetes probe model](https://kubernetes.io/docs/concepts/workloads/pods/probes/):
liveness failure can restart a container, while readiness failure removes it
from service traffic without restarting it.

| Endpoint | Success response | Failure response | Purpose |
| --- | --- | --- | --- |
| `GET /livez` | `200 {"status":"alive"}` | No application-level failure status; an unresponsive server fails the caller's probe | Startup and liveness |
| `GET /readyz` | `200 {"status":"ready"}` | `503 {"status":"not_ready"}` | Traffic readiness |
| `GET /health` | `200 {"name":"stargate","version":"<version>","status":"healthy"}` | `503` with the same fields and `status: "unhealthy"` | Compatibility with existing health clients |
| `GET /admin/health` | `200` with component status, latency, backend kinds, and timestamp | `503` with component diagnostics | Authenticated operator diagnostics |

The three public endpoints also support `HEAD`, return `Cache-Control: no-store`,
and bypass authentication, API rate limiting, and gateway routing policies.
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
`/admin/health` uses the same bounded dependency checks.

Initialization includes database migrations, state-store setup, signing
configuration, and the compiled gateway configuration. The listener opens
after initialization, so `/livez` also works as a startup probe. Fatal startup
errors still prevent the server from listening.

Individual gateway upstreams, SMTP, external identity providers, telemetry,
and audit relay delivery lag do not gate pod readiness. Invalid hot reloads
retain the last valid configuration and keep the current readiness behavior.

## Shutdown

On SIGTERM or SIGINT, Stargate marks readiness false immediately. It keeps
the listener open for `SERVER_DRAIN_DELAY_SECS` (default `5`, `0` disables the
delay), allowing traffic already in transit to finish reaching the pod while
load balancers learn that it is unavailable. During this window `/livez`
returns `200` and `/readyz` and `/health` return `503` without dependency I/O.
An in-flight readiness check rechecks the lifecycle before returning success.

After the delay, the server closes listeners and drains active connections,
and cancels active gateway requests, mirrors, and WebSockets. HTTP and TLS
connections share the `SERVER_SHUTDOWN_TIMEOUT_SECS` deadline (default `25`);
remaining connection tasks are cancelled when it expires. Tracked gateway tasks
are drained before the audit relay stops and the edge memory store is persisted.
The deployment termination grace period remains the final process deadline.

Allow room in the pod's `terminationGracePeriodSeconds` for the delay,
connection draining, and persistence hooks. The Helm chart defaults to 40
seconds. Tune the delay to your load balancer's propagation behavior; it is
not a guarantee that every load balancer has stopped routing traffic.

## Helm and migration

The chart uses `/livez` for startup/liveness and `/readyz` for readiness and
the Helm connection test. Custom `probes.*` overrides remain supported.
The default three-second probe timeout exceeds the one-second dependency
deadline. Probe intervals and failure thresholds determine how quickly
Kubernetes reacts to failures.

Existing `/health` clients retain the same JSON shape, but the endpoint now
also reflects Redis availability in cluster mode and lifecycle readiness.
Move any liveness checks still pointing at `/health` to `/livez`. Deploy an
image containing these endpoints when upgrading the chart defaults.
