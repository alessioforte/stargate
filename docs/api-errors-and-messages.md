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
   successful message to `src/etc/msg.rs`.
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
| `gateway.overloaded` | 503 | No primary concurrency permit is available |
| `gateway.replay_memory_exhausted` | 503 | Required failover replay cannot reserve storage |
| `gateway.upload_timeout` | 408 | Upload/replay buffering makes no data progress |
| `gateway.timeout` | 504 | Connection, headers, or absolute finite budget expires |
| `gateway.cancelled` | 503 | Gateway work is cancelled during shutdown |

Timeout/cancellation responses may contain a safe `phase` parameter. Once headers
have been sent, a body/session timeout closes the stream instead of sending a
second JSON response or retrying. Mirror capacity/memory saturation skips optional
mirror traffic. See [runtime budgets](config-v1.md#runtime-deadlines-and-resource-budgets).
