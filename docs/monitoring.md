# Monitoring Stargate

Stargate exports traces, metrics and logs through OpenTelemetry Protocol (OTLP).
The contract is the OpenTelemetry instrument and attribute names below.
Dashboards, alert rules, receivers and notification routing belong to the
operator's infrastructure repository. Stargate installs no backend-specific
observability resources.

## Collector

Use a private collector endpoint, configure its authentication/TLS and restrict
network access. A backend-neutral forwarding example:

```yaml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317
      http:
        endpoint: 0.0.0.0:4318
processors:
  batch: {}
exporters:
  otlp/backend:
    endpoint: telemetry-backend.example:4317
    tls:
      ca_file: /etc/collector/backend-ca.pem
service:
  pipelines:
    traces:
      receivers: [otlp]
      processors: [batch]
      exporters: [otlp/backend]
    metrics:
      receivers: [otlp]
      processors: [batch]
      exporters: [otlp/backend]
    logs:
      receivers: [otlp]
      processors: [batch]
      exporters: [otlp/backend]
```

Set Stargate's `OTEL_EXPORTER_OTLP_ENDPOINT` and protocol to the receiver.
The SDK's default metric export interval is 60 seconds. Allow an export interval
before diagnosing an absent instrument. Counters without observations may be
absent. A backend may translate instrument names, attribute names and units;
use its translation rules when implementing the catalog, rather than assuming
the OTLP names are unchanged. Monitor collector health and failed exports
separately from application telemetry.

## Resource identity and aggregation

| Resource attribute | Meaning |
| --- | --- |
| `service.name` | `OTEL_SERVICE_NAME`, default `stargate` |
| `service.version` | Binary version |
| `service.instance.id` | Distinct process identity; chart uses pod name, otherwise an explicit value or a process-lifetime ULID |
| `deployment.environment.name` | Operator-defined environment, e.g. `production`, through `OTEL_RESOURCE_ATTRIBUTES` |

For example, outside the chart:

```sh
OTEL_RESOURCE_ATTRIBUTES=deployment.environment.name=production
```

The chart manages resource attributes from downward-API pod/namespace fields.
Add environment identity in the collector's resource processing when the chart
owns the variable. Keep identity through ingestion: compute rates of cumulative
counters **per instance before aggregating**. Do not merge cumulative streams
from different processes. Instrument dimensions are bounded states and names
from configuration; no URL, user ID or request ID becomes a metric dimension.
Keep these out of operator-supplied resource attributes as well.

Unless marked global below, each value is per replica. Audit backlog gauges
repeat the same global SQL or stream state on replicas: aggregate with **max**,
not sum. Counters such as audit publication/pruning represent work performed by
each replica, so their rates can be summed. Aggregate histogram buckets before
computing quantiles; do not average per-replica quantiles.

## HTTP server spans

Names and attributes follow the stable HTTP conventions in
[OpenTelemetry semantic conventions v1.45.0](https://github.com/open-telemetry/semantic-conventions/tree/v1.45.0/docs/http).
The initial operation name is the known method; unknown methods use `HTTP` as
the operation name and `_OTHER` as the method attribute. Once known, the name
becomes `METHOD route` (or `HTTP route` for an unknown method).

IAM routes use Axum's `MatchedPath`, including parameter placeholders such as
`/admin/users/{id}`. Probes retain `/livez` and `/readyz`; filter them in the
collector if desired. Gateway operations record the configured `stargate.router`;
`http.route` is set only from a positive configured exact or template path.
Prefix/regex matchers and unmatched requests do not copy the actual path into
the operation name or route attribute. Router names remain bounded by configuration.

| Span attribute | Value |
| --- | --- |
| `http.request.method` | `CONNECT`, `DELETE`, `GET`, `HEAD`, `OPTIONS`, `PATCH`, `POST`, `PUT`, `QUERY`, `TRACE`, or `_OTHER` (case-sensitive) |
| `http.request.method_original` | Original method only when normalized; span only |
| `http.route` | Matched low-cardinality template/exact path, when known |
| `url.path` | Actual path; span only |
| `url.scheme` | `http` or `https` from listener/public HTTPS settings |
| `server.address`, `server.port` | Original request authority/Host, before gateway middleware; scheme's default port if omitted |
| `client.address` | Client IP resolved through configured trusted proxies |
| `network.peer.address` | Immediate connection peer IP, without port |
| `user_agent.original` | User-Agent when present |
| `http.response.status_code` | Response status number |
| `error.type` | HTTP 5xx status as a string, or a bounded gateway error code for an internal/transport failure |
| `request_id` | Stargate request ULID; also used in JSON logs |
| `trace_id` | Valid SDK trace ID when telemetry is active |

Optional unavailable attributes are omitted. No query string is recorded on
request spans or JSON logs: neither `url.query` nor a legacy query attribute is
emitted, including OAuth callbacks. Only server failures set span status ERROR;
4xx responses keep server span status unset. gRPC may represent an internal or
transport failure in RPC status while HTTP is 200; its bounded error code still
identifies that server failure.

## Metric catalog

`C` means monotonic counter, `H` histogram and `G` observable gauge, except the
active gateway resource up/down counter which is gauge-like. Durations are
**seconds (`s`)**. Count/gauge quantities in the unit column describe their
meaning; unless `s` or `By` is shown, their OTLP unit field is unset.
Request latency measures handler/response-header completion;
custom gateway transfer durations cover long-lived body/WebSocket lifetimes.

| OpenTelemetry instrument | Type / unit | Attributes and allowed values | Meaning / scope |
| --- | --- | --- | --- |
| `http.server.request.duration` | H / s | `http.request.method` normalized as above; numeric `http.response.status_code`; `http.route` when known; `url.scheme=http` or `https`; `error.type` on failures; `stargate.request.kind=iam` or `gateway` | All inbound requests; count requests from histogram observations, separately by kind |
| `stargate.gateway.requests` | C / requests | Configured `stargate.router`, `stargate.service` or `unmatched`; response status | Gateway-only subset; do not add it to the HTTP histogram count |
| `stargate.gateway.duration` | H / s | Same as gateway requests | Through gateway response headers |
| `stargate.gateway.transfer.duration` | H / s | Bounded `phase`; `stargate.outcome` from transfer completion/cancellation/error handling | Body/WebSocket lifetime |
| `stargate.gateway.upstream.requests` | C / attempts | Configured `stargate.service`; `stargate.target_kind=upstream` or `direct_response`; `network.protocol.name=http` or `ws`; status; `stargate.outcome=success` or `error` | Dispatch attempts |
| `stargate.gateway.upstream.duration` | H / s | Same as upstream requests | Dispatch through response headers |
| `stargate.gateway.upstream.targets` | G / targets | Configured `stargate.upstream`; `stargate.state=healthy`, `unhealthy`, `circuit_open` | Active-generation targets, per replica |
| `stargate.gateway.failovers` | C / transitions | `stargate.reason=status`, `transport`, or `protocol`; response status | Failover transitions |
| `stargate.gateway.mirrors` | C / dispatches/outcomes | Bounded `stargate.outcome` | Mirror activity |
| `stargate.gateway.grpc.responses` | C / responses | Configured `router`, `service`; numeric `grpc_status` | Completed RPC statuses |
| `stargate.gateway.resources.active` | Up/down / slots or bytes | `kind=primary`, `mirror`, `replay_memory` | Active requests/mirrors or reserved replay bytes |
| `stargate.gateway.rejections` | C / rejections | `kind=primary`, `replay_memory`, `ingress`, `signing` | Capacity rejections |
| `stargate.gateway.timeouts` | C / timeouts | Bounded `phase` (`total`, `upload`, `ingress`, `connect`, `response_headers`, `response_body`, `disposal`, `websocket_handshake`, `websocket_idle`) | Timeout phases |
| `stargate.gateway.replay.bytes` | H / By | none | Replay-buffered body size |
| `stargate.gateway.response.disposals` | C / bodies | `stargate.disposal.kind`; `stargate.outcome` from bounded disposal results | Discarded response outcomes |
| `stargate.gateway.response.disposal.bytes` | H / By | Same as disposals | Observed discarded data bytes |
| `stargate.gateway.response.disposal.duration` | H / s | Same as disposals | Bounded body disposal time |
| `stargate.gateway.policy.decisions` | C / decisions | `stargate.policy_kind=auth`, `access_control`, `ingress`, `rate_limit`, `quota`; `stargate.outcome=allowed`, `denied`, `error` | Policy decisions |
| `stargate.gateway.propagation.drafts` | C / drafts | Bounded `stargate.outcome` and `stargate.kind` | Internal-context draft outcomes |
| `stargate.gateway.internal_context.issues` | C / issues | Configured service; `stargate.dispatch.kind=primary` or `mirror`; bounded outcome/reason | Internal-context signing outcomes |
| `stargate.gateway.internal_context.signing.duration` | H / s | Same as issues | CPU signing duration, including sub-millisecond work |
| `stargate.gateway.internal_context.queue.duration` | H / s | Configured service and dispatch kind | Signing queue wait |
| `stargate.gateway.internal_context.token.size` | H / By | Same as issues | Compact signed token size |
| `stargate.limits.store_errors` | C / failures | `point=gateway_ingress`, `iam_ingress`, `admin_principal`, `rate_limit`, `quota`; `outcome=allowed` or `denied` | Traffic store errors/timeouts, per replica |
| `stargate.config.reloads` | C / activations | `stargate.config_kind=gateway_config` or `policies`; `stargate.outcome=success`, `error`, `transport_error`, `internal_context_error` | Activation outcomes |
| `stargate.db.pool.connections` | G / connections | `stargate.state=idle` or `in_use` | Actual initialized SQL pool sample |
| `stargate.db.pool.max` | G / connections | none | Configured maximum of that pool |
| `stargate.audit.outbox.pending` | G / events | none | Global unpublished SQL backlog; max across replicas |
| `stargate.audit.outbox.oldest_pending_age` | G / s | none | Global oldest unpublished age; max |
| `stargate.audit.stream.length` | G / entries | none | Global Redis stream length; max |
| `stargate.audit.stream.oldest_unconfirmed_age` | G / s | none | Global oldest pending/undelivered age; max; absent without groups |
| `stargate.audit.relay.paused` | G / 0 or 1 | `stargate.reason=consumer_backlog` or `group_missing` | Per-replica pause state; max detects any pause |
| `stargate.audit.relay.records` | C / events | `stargate.outcome=success` or `error` | Publications by this replica |
| `stargate.audit.relay.duration` | H / s | Same as relay records | Relay batch duration |
| `stargate.audit.relay.errors` | C / errors | `stargate.outcome=error` | Failed relay batches |
| `stargate.audit.outbox.pruned` | C / rows | none | Retention work by this replica |

SQL pool sampling is non-transactional. Idle is clamped to the sampled pool size
because acquisition/release can race the atomic reads. Maximum capacity comes
from the initialized pool options, not another environment lookup. PostgreSQL's
notification listener reserves one connection **inside** this pool; it adds no
external connection.

Upstream states are mutually exclusive: healthy means closed; circuit_open means
open; unhealthy means half-open/probing. Observation never advances state or
reserves a probe. A failed attempt below the threshold still leaves a closed
target. Used upstreams shared by several leaf services are counted once. Every
collection reads the active snapshot; unused upstreams without a balancer are
omitted, and pinned old requests do not keep obsolete generations in the gauge.

Duration histogram boundaries are explicit, in seconds:

- HTTP server: `[0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1, 2.5, 5, 7.5, 10]`, as recommended by the HTTP conventions.
- Gateway request/upstream/disposal and audit relay: `[0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10, 30, 60, 120]`.
- Transfer: `[0.01, 0.1, 0.5, 1, 5, 10, 30, 60, 300, 900, 3600]`.
- Signing: `[0.000001, 0.000005, 0.00001, 0.000025, 0.00005, 0.0001, 0.00025, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 1]`.
- Signing queue: `[0.00001, 0.00005, 0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1, 2.5, 5]`.

An overflow bucket captures larger values. Byte histograms remain in `By`.

## Alert catalog

Implement these conditions in the chosen backend. Rates are per-instance before
summing. The duration column is how long a condition must remain true; a dash
means evaluate the indicated window/value directly. Alerts are recommendations,
not additional readiness conditions.

| Alert | Instrument / attributes | Aggregation | Threshold / duration | Severity | First action / planned runbook section |
| --- | --- | --- | --- | --- | --- |
| High HTTP error rate | `http.server.request.duration`, status ≥ 500, grouped by `stargate.request.kind` | Error observation rate / all observation rate, separately for IAM and gateway | > 2% / 10m | critical | Inspect dependency health and status distribution; `high-http-error-rate` |
| Limit store degraded | `stargate.limits.store_errors` | Sum of per-instance rates | Increasing / 5m | warning | Check Redis connectivity/latency; `redis-unavailable` |
| Limit store denying | Same instrument, `outcome=denied` | Sum of rates | Increasing / 5m | critical | Restore Redis or review failure policy; `redis-unavailable` |
| Configuration activation failed | `stargate.config.reloads`, `stargate.outcome` not `success` | Sum of increments over 15m | > 0 / — | warning | Inspect failed revision and retained snapshot; `configuration-activation-failed` |
| Audit relay errors | `stargate.audit.relay.errors` | Sum of rates | Increasing / 10m | warning | Inspect SQL/Redis errors, never delete pending events; `audit-delivery-delayed` |
| Old unpublished audit event | `stargate.audit.outbox.oldest_pending_age` | Max across replicas | > 300s warning; > 1800s critical / — | warning / critical | Restore relay/consumer; `audit-delivery-delayed` |
| Audit relay paused | `stargate.audit.relay.paused` | Max across replicas/reasons | = 1 / 10m | critical | Inspect missing group or protected capacity; `audit-delivery-delayed` |
| Audit event unconfirmed | `stargate.audit.stream.oldest_unconfirmed_age` | Max across replicas | > 900s / — | warning | Recover pending entries before acknowledgement; `audit-delivery-delayed` |
| Upstream circuit open | `stargate.gateway.upstream.targets`, `stargate.state=circuit_open` | Per upstream and replica | > 0 / 5m | warning | Inspect backend, probes and TLS; `upstream-unavailable` |
| Upstream unavailable | Same instrument, `stargate.state=healthy` | Per upstream/replica, with configured target count > 0 | = 0 / 2m | critical | Restore a usable backend; `upstream-unavailable` |
| Gateway rejecting work | `stargate.gateway.rejections` | Sum of rates | Increasing / 5m | warning | Inspect rejection kind and resource limits; `gateway-overloaded` |
| SQL pool saturated | `stargate.db.pool.connections`, `stargate.state=in_use`; `stargate.db.pool.max` | In-use / max, per replica | > 0.9 / 5m | warning | Check slow transactions and connection capacity; `postgresql-unavailable` |

Runbook names refer to planned sections of [Operations runbook](operations-runbook.md).
Notification routing and delivery tests are operator-owned. Revisions are not
metric dimensions: compare `config.gatewayRevision` and `config.policiesRevision`
on each replica's `/admin/health` after changes or a restart-required save.

Also monitor infrastructure outside Stargate: available replicas, pod restarts
and memory through Kubernetes collector receivers, certificate expiry through
the ingress controller or certificate manager, and collector/backend health.
These are not Stargate instruments and are not installed by its chart.

## Recommended views

Build views in the operator's backend using the OTLP catalog:

- HTTP request counts, latency p50/p95/p99 and error ratio, separately for IAM and gateway, using `http.server.request.duration` and `stargate.request.kind`.
- Upstream attempts, failover, response latency and target states by configured upstream and replica.
- Traffic limit store error rates by point and outcome.
- Global audit backlog, oldest unpublished/unconfirmed ages, stream length and relay pause/error rates; see [Audit delivery](audit-events.md).
- Configuration activation rate by kind and outcome; compare active revisions separately when diagnosing drift.
- SQL pool idle/in-use counts and utilization, preserving instance identity.
- Gateway primary/mirror occupancy, reserved replay bytes, rejection and timeout rates.
