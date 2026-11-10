# Audit event delivery

Stargate writes an immutable JSON event to SQL's `outbox_events` in the same
transaction as the mutation. In cluster deployments each replica relays batches
to Redis Streams. The consumer and its archive are separate services. Edge has
no relay: inspect events through `GET /admin/outbox-events` and
`GET /admin/outbox-events/{event_id}`.

## Consumer contract

Each stream entry has exactly one field, `payload`, containing the stored JSON
verbatim. Stream IDs identify deliveries; `event_id` identifies the event.
Delivery is at least once: a publish followed by a failed SQL commit can deliver
the same payload again. Deduplicate durably by `event_id` before acknowledging.
Each claimed batch is ordered by SQL `seq`; batches from different replicas
can interleave. There is no global delivery order. Use `occurred_at` for event
time ordering if needed, allowing ties and delayed deliveries.

Event version 1 has the following fields. Optional values are explicit JSON
`null` unless otherwise noted; consumers should tolerate new metadata keys.

| Field | Type and meaning |
| --- | --- |
| `event_id` | Uppercase 26-character Crockford Base32 ULID |
| `event_version` | Integer, currently `1` |
| `occurred_at` | RFC 3339 UTC timestamp |
| `scope` | Object: `kind` is `application`, `control_plane`, or `organization`; the last also has an `organization_id` ULID |
| `operation_id` | Optional ULID correlating events from one operation; no pairing semantics |
| `actor` | Object: `type`, optional `id`, and `email` (always null in version 1) |
| `service` | Object with `name: "stargate"` |
| `resource` | Object with `type`, `id`, optional `name` |
| `action` | Resource-specific action, such as `user.created` |
| `operation` | `create`, `update`, `delete`, `restore`, `archive`, or a custom operation string; Stargate validates action/operation pairs |
| `before`, `after` | Optional JSON snapshot objects, sanitized by the producer |
| `metadata` | JSON object (empty when absent), including safe source/correlation facts |
| `request` | Optional object: `request_id`, optional `trace_id`, `ip_address`, `user_agent` |

Allowed `actor.type` values are `admin`, `admin_key`, `anonymous`, `api_key`,
`external_identity`, `service`, `system` and `user`. Resource types are
`admin_key`, `api_key`, `config_document`, `credential`, `oauth_client`,
`oauth_consent`, `organization`, `organization_membership`, `service_account`,
`super_admin` and `user`. Administrative events use `control_plane`.
Login/logout do not produce audit events. Secrets and full configuration
contents are excluded from producer snapshots. Saving a user's own language emits
`user.updated` in application scope with `source: account`, `changed_fields:
["attrs.locale"]`, and before/after snapshots containing only `locale`. An
unchanged language emits no event.

Limits are UTF-8 serialized bytes: whole event 65,536; each before/after
snapshot 16,384; metadata 8,192; snapshot string 2,048; snapshot array 64 items;
dynamic JSON nesting 8 levels. Type/action fields are at most 128 bytes,
custom operations 64, identifiers and resource names 255, actor email 320
(but prohibited in version 1), IP address 64 and user agent 512. Identifier, type and request fields reject
ASCII control characters. Request trace IDs use the same 255-byte identifier limit; HTTP producers
supply the valid OpenTelemetry trace ID when available. The validated producer contract lives in
`crates/db/src/ent/raw_audit.rs`.

This example is the control-plane fixture exercised by
`audit_golden_fixtures_match_the_exact_serialized_payload`:

```json
{
  "event_id": "01JZ0000000000000000000003",
  "event_version": 1,
  "occurred_at": "2026-07-18T10:00:02Z",
  "scope": {
    "kind": "control_plane"
  },
  "operation_id": null,
  "actor": {
    "type": "admin",
    "id": "01JZ000000000000000000000D",
    "email": null
  },
  "service": {
    "name": "stargate"
  },
  "resource": {
    "type": "user",
    "id": "01JZ000000000000000000000E",
    "name": null
  },
  "action": "user.created",
  "operation": "create",
  "before": null,
  "after": {
    "email": "user@example.com",
    "nickname": "example"
  },
  "metadata": {
    "source": "admin_api"
  },
  "request": {
    "request_id": "request-control-plane-1",
    "trace_id": null,
    "ip_address": null,
    "user_agent": "example-client/1.0"
  }
}
```

## Configure protected delivery

Use Redis/Valkey **6.2 or newer**. Before starting Stargate, create every
configured consumer group at the beginning of the stream:

```sh
redis-cli XGROUP CREATE audit.raw audit-archiver 0 MKSTREAM
```

Set `AUDIT_STREAM_CONSUMER_GROUPS=audit-archiver` on every replica. Multiple
names are comma-separated, distinct and nonempty. A missing stream or group
pauses delivery until it exists, so creating it just after startup also works.
Keep all replicas configured consistently and use dedicated stream keys.
Consumers use `XREADGROUP` and issue `XACK` **only after durable archival**.
Recover abandoned pending entries with `XAUTOCLAIM`; explicitly archive poison
messages in a dead-letter destination before acknowledging them.

For every configured group, Stargate protects its oldest pending ID, or its
last-delivered ID when no pending entries remain. It trims with
`XTRIM MINID ~` below the minimum protected position across all groups.
Acknowledgements may arrive out of order: a stuck old entry still blocks
trimming. New entries use `XADD` without trimming. Neither manual stream
trimming/deletion nor `XGROUP SETID` rewinds are covered by this guarantee.
Adding a new group after data has already been trimmed cannot recover it;
plan group membership before publication.

Before claiming a batch, the relay pauses if stream length plus
`max(MAXLEN / 10, 4 × AUDIT_RELAY_BATCH)` reaches `AUDIT_STREAM_MAXLEN`.
Trimming continues during the pause, and acknowledgements automatically let
delivery resume. Concurrent replicas and approximate trimming can temporarily
exceed the target: it is a capacity target, not an exact byte or entry cap.
Size for event rate × tolerated consumer outage, plus the margin and Redis
entry overhead. Events beyond stream capacity wait durably in PostgreSQL;
size and monitor its storage too. A single unacknowledged entry can pause the
whole relay: alert on its age and fix the consumer rather than deleting it.

With **no configured groups**, legacy `XADD MAXLEN ~` behavior can lose events
before consumers archive them. `AUDIT_STREAM_MAXLEN=0` removes the capacity
pause; protected trimming still runs with groups, but Redis memory can grow
without bound while consumers lag. Without groups and with zero max length,
there is no stream trimming at all.

## Diagnostics and retention

`/admin/health` exposes cached cluster-only `audit` diagnostics: `relay`
(`running`, `paused`, `failing`, `disabled`), optional `pauseReason`
(`consumer_backlog`, `group_missing`), `pendingEvents`,
`oldestPendingAgeSecs`, optional `streamLength` and
`oldestUnconfirmedAgeSecs`, plus `updatedAt`. Absent stream measurements mean
not yet known, not zero. Disabling the relay leaves its diagnostics disabled
without background queries. Audit backlog does not change readiness or the
overall health status. The Console displays the same diagnostics.

Observable OTLP gauges:

| Metric | Unit |
| --- | --- |
| `stargate.audit.outbox.pending` | Events |
| `stargate.audit.outbox.oldest_pending_age` | Seconds |
| `stargate.audit.stream.length` | Entries |
| `stargate.audit.stream.oldest_unconfirmed_age` | Seconds |
| `stargate.audit.relay.paused` | 0/1, bounded `stargate.reason` label |

Outbox gauges describe the same global backlog on every replica: aggregate
with **max**, never sum. Stream gauges also describe the shared stream.
`oldest_unconfirmed_age` is zero when groups are caught up and absent without
groups. Pause logs occur on entry and recovery. Redis/SQL command errors set
`failing` and use retry backoff. Existing relay records/duration/errors metrics
remain available. `stargate.audit.outbox.pruned` counts deleted rows per replica;
its increments can be summed.

Cluster defaults to `AUDIT_OUTBOX_RETENTION=30d`; a tools-format duration
(`ns`, `us`, `ms`, `s`, `m`, `h`, `d`, `w`) or `none` is accepted. At most every
ten minutes per replica, maintenance deletes published rows older than this
duration in batches of 1,000, skipping locked rows. **Unpublished rows are
never removed.** Retention is measured from publication, not event creation.
After pruning, admin lists omit those rows and lookup by ID returns 404.
The stream and archive have independent retention; SQL maintenance never
removes Redis entries. Edge performs no pruning. The SQL outbox is a delivery
buffer, not a permanent audit archive. Choose archival retention separately.
