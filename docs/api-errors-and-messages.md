# API Errors and Messages

Stargate exposes stable, domain-specific codes for errors and successful
message responses. Clients use the code for behavior and localization; the
English `message` remains a safe fallback.

## Error envelope

```json
{
  "message": "User not found",
  "code": "user.not_found",
  "type": "not_found",
  "link": "https://docs.example.com/errors/user.not_found",
  "params": {
    "id": "01J..."
  }
}
```

- `code`: stable lookup key and public API contract.
- `type`: broad category for filtering and presentation.
- `message`: safe English fallback. Internal causes are never returned here.
- `link`: documentation URL for the exact code.
- `params`: optional, safe values used to interpolate localized templates.

Successful message responses use the smaller shape:

```json
{
  "message": "User deleted successfully",
  "code": "user.deleted",
  "params": {
    "id": "01J..."
  }
}
```

OAuth/OIDC endpoints keep their RFC-defined error envelope when the protocol
requires it.

## Catalog endpoints

`GET /i18n/{locale}` returns a complete catalog:

```json
{
  "schema": "stargate/i18n/v1",
  "locale": "it",
  "fallbackLocale": "en",
  "version": "<content sha256>",
  "messages": {
    "errors": {},
    "messages": {}
  }
}
```

English strings are generated from the Rust definitions. Other locales contain
only authored overrides internally and fall back to English when the public
catalog is built. The response supports `ETag`/`If-None-Match` and includes
`Content-Language`.

`GET /docs/errors` returns the complete error metadata used to build a
documentation UI: code, HTTP status, type, English fallback, and link.

## Adding or changing a code

1. Add the semantic variant and metadata to `src/err/types.rs`, or add a
   successful message to `src/etc/http/messages.rs`.
2. Use the typed variant at the call site. Do not construct string codes.
3. Add only the locale overrides that have been translated.
4. Add safe interpolation values with `with_param`; never include secrets or
   internal error causes.
5. Run the catalog tests. They verify uniqueness, complete published catalogs,
   valid override keys, and stable content versions.

Codes may be added, but an existing code must not be renamed or reused for a
different condition once clients depend on it.

## Gateway lifecycle errors

| Code | HTTP status | Condition |
| --- | --- | --- |
| `gateway.overloaded` | 503 | Primary admission or internal-context signing capacity is exhausted |
| `gateway.ingress_rate_limit_exceeded` | 429 | The client IP exhausted its gateway ingress allowance, before authentication or routing |
| `gateway.ingress_unavailable` | 503 | The ingress limiter is missing, fails, or exceeds its store deadline |
| `gateway.replay_memory_exhausted` | 503 | Required failover replay cannot reserve storage |
| `gateway.upload_timeout` | 408 | Upload/replay buffering makes no data progress |
| `gateway.request_body_failed` | 400 | The client request body fails during streaming or replay buffering |
| `gateway.timeout` | 504 | Connection, headers, or absolute finite budget expires |
| `gateway.cancelled` | 503 | Gateway work is cancelled during shutdown |

Signing overload responses contain `phase=signing` and reason
`signing_queue_full` or `signing_queue_timeout`; they contact no upstream and do
not trigger failover. Timeout/cancellation responses may contain a safe `phase`
parameter. Once headers have been sent, a body/session timeout closes the stream instead of sending a
second JSON response or retrying. Mirror capacity/memory saturation skips optional
mirror traffic. See [runtime budgets](config-v1.md#runtime-deadlines-and-resource-budgets).

Ingress throttling includes integer-second `Retry-After`, `X-RateLimit-Limit`,
`X-RateLimit-Remaining`, and `X-RateLimit-Scope: ingress`. An allowed admission
leaves response limit headers to the selected resource policies. Probes bypass
both ingress admission and IAM rate-limit middleware.

All ingress, resource rate/quota, and IAM/admin `429` responses use decimal
seconds in `Retry-After`, rounding fractional seconds upward; a missing limiter
duration defaults to `60` seconds. Rate/quota metadata is retained. All `503`
errors, including gateway overload, retain their `10`-second delay. See
[rate and quota policies](config-v1.md#policies) for the header contract.

Request-body failures and `gateway.request_preparation_failed` (500), including
invalid upstream URIs and internal-context failures, stop dispatch without
failover or upstream-health penalties. Eligible operations may fail over on
`upstream.connection_failed` (502) or configured upstream response statuses;
ordinary mutations return the first result. See [replay eligibility](config-v1.md#automatic-replay-and-mutations).
