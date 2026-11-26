# Listeners

Stargate binds one or more listeners at startup. Each listener has a name, a
complete address, the serving planes it exposes, its own TLS switch, its proxy
trust, and its own connection cap. Without configuration there is one implicit
listener, which is the single-port behavior every deployment had before named
listeners.

Listeners are process settings: they come from the environment, need a restart,
and never appear in the `stargate/v1` gateway document.

## The implicit listener

With `LISTENERS` unset the process opens `0.0.0.0:$PORT` (default `5050`) and
serves every plane on it:

| Setting           | Default | Meaning                                                     |
| ----------------- | ------- | ----------------------------------------------------------- |
| `PORT`            | `5050`  | Port of the implicit listener                               |
| `TLS_ENABLED`     | `false` | Terminates TLS on the implicit listener                     |
| `MAX_CONNECTIONS` | `1024`  | Concurrent connections the implicit listener accepts         |
| `TRUSTED_PROXIES` | none    | Proxy sources whose forwarding headers it accepts            |

`TLS_CERT` and `TLS_KEY` hold the certificate when TLS is on. Routes, proxies,
and hosted apps behave exactly as they did before named listeners.

## Named listeners

```text
LISTENERS=public,admin,internal,management
LISTENER_PUBLIC_ADDRESS=0.0.0.0:5050
LISTENER_PUBLIC_PLANES=identity,gateway
LISTENER_PUBLIC_TRUSTED_PROXIES=10.42.0.0/16
LISTENER_ADMIN_ADDRESS=127.0.0.1:5051
LISTENER_ADMIN_PLANES=admin
LISTENER_INTERNAL_ADDRESS=[::1]:5052
LISTENER_INTERNAL_PLANES=admin
LISTENER_INTERNAL_TLS=off
LISTENER_INTERNAL_MAX_CONNECTIONS=256
LISTENER_MANAGEMENT_ADDRESS=0.0.0.0:9000
LISTENER_MANAGEMENT_PLANES=management
```

`LISTENERS` is a comma-separated list of names. For each name:

| Setting                           | Required | Default                     |
| --------------------------------- | -------- | --------------------------- |
| `LISTENER_<NAME>_ADDRESS`         | yes      | –                           |
| `LISTENER_<NAME>_PLANES`          | yes      | –                           |
| `LISTENER_<NAME>_TLS`             | no       | on, except for management   |
| `LISTENER_<NAME>_MAX_CONNECTIONS` | no       | `1024`                      |
| `LISTENER_<NAME>_TRUSTED_PROXIES` | no       | none                        |

Names are 1–32 ASCII letters or digits, start with a letter, and are compared
case-insensitively; the environment variable uses the uppercase name. A field
no declared listener reads stops startup, so a misspelled name or field cannot
silently fall back to a default.

The address is a literal IP and a port — `0.0.0.0:5050`, `127.0.0.1:5051`,
`[::1]:5052`. Hostnames and addresses without a port are rejected.

`PORT`, `TLS_ENABLED`, `MAX_CONNECTIONS`, and `TRUSTED_PROXIES` configure the
implicit listener and stop startup when `LISTENERS` is declared: every listener
states its own values.

### Planes

On IAM hosts, listeners without `admin` reserve `/admin` and the configured
Console path (including descendants) with `404`; a gateway catch-all cannot
forward them. Other hosts keep gateway routing. Identity and admin listeners
serve `/docs`, containing only the planes that listener serves. Admin API
credentials must use `Authorization: Bearer` or `x-api-key` headers.

| Plane        | Routes                                                                 |
| ------------ | ---------------------------------------------------------------------- |
| `management` | `/livez`, `/readyz`                                                    |
| `identity`   | OAuth/OIDC provider, accounts, sign-up, social sign-in, hosted auth app, OpenAPI, i18n, internal-context JWKS |
| `admin`      | `/admin/*` and the Console                                             |
| `gateway`    | everything the gateway configuration matches                           |

A plane may appear on more than one listener; each listener mounts only the
planes it declares. On a listener with IAM planes the shared ingress allowance
and CORS/compression apply as on the single port. The identity plane also
admits the Console's public origin (`CONSOLE_PUBLIC_URL`) on `/oauth/token` and
`/oauth/revoke` only, so a Console served by the admin listener can exchange
codes and refresh tokens against the issuer; `/admin` stays on the Console's
own origin. On the routes whose credential comes from the session token
(profile, logout, authenticated MFA, organization switch, consents,
organizations), a request carrying `Authorization` skips the browser origin
guard and the cookie is never used alongside such a header. Routes without a
header credential (login, OTP/MFA login, reset, refresh token, sign-up, social
state, `/oauth/login`, interaction reads, consent, logout confirmation) keep
the full guard even with `Authorization`. The gateway plane owns its paths,
preflights, and encoding.

Host selection (`IAM_HOSTS`) still applies per request. On a listener that
serves both IAM planes and the gateway, a request on an IAM host falls through
to the gateway when no IAM route matches, exactly as on the implicit listener;
the gateway receives the path the client sent. A listener that serves IAM
planes without the gateway answers `404` for paths no IAM route matches, so a
public listener does not forward `/admin` to a backend.

The management plane must be alone on its listener: probes carry no secret and
stay in clear text, while the other planes can terminate TLS. `LISTENER_<NAME>_TLS=on`
with only the management plane stops startup.

### Proxy trust

A listener trusts no forwarding header by default. Only a peer in its
`LISTENER_<NAME>_TRUSTED_PROXIES` list may set `X-Forwarded-For`, `X-Real-IP`, or
`Forwarded`; the client address is resolved from `X-Forwarded-For` (else
`X-Real-IP`) once at ingress and every reader uses it (logs and spans, ingress and
admin limits, brute-force guards, OTP, source-IP matching, audit, and the signed
internal context). The same plane can serve an ingress listener and an internal
listener with different trust: the internal listener keeps the peer address and
removes `Forwarded`, `X-Real-IP`, and every `X-Forwarded-*` field before routing,
so an internal service cannot forge its address in logs, limits, or audit.

Entries are IP addresses or CIDR blocks. Invalid entries stop startup instead of
being dropped, and the list is read once. `TRUSTED_PROXIES` is the implicit
listener's list; with `LISTENERS` it stops startup so the trust has one source.

### TLS

All TLS listeners share one certificate (`TLS_CERT`, `TLS_KEY`, PKCS#8). Each
listener offers ALPN `h2` and `http/1.1`, so TLS clients negotiate HTTP/2,
including gRPC. A TLS listener without a certificate stops startup.

HTTPS is decided per request, not per process: a request is HTTPS when its
listener terminates TLS or when `JWT_ISSUER` or `OAUTH_BASE_URL` is an
`https://` public URL (a proxy terminates TLS). Cookies get `Secure`, responses
carry HSTS, and the request's own origin and span scheme are `https://` only on
those responses. With one listener in clear text and one with TLS and an
`http://` public URL, only the TLS listener answers with `Secure` and HSTS.

## Connection protection

Every listener applies the same per-connection limits:

| Limit                          | Value   | Why                                                                 |
| ------------------------------ | ------- | ------------------------------------------------------------------- |
| First request and header read  | 10 s    | Ends a connection that sends no complete request                     |
| TLS handshake timeout          | 10 s    | A client that never completes the handshake releases its slot       |
| HTTP/2 concurrent streams      | 100     | Bounds work per connection; clients wait or open another one        |
| HTTP/2 header list size        | 32 KiB  | Covers cookies and bearer tokens with headroom; bounds decode work  |
| HTTP/2 keep-alive              | 30 s / 10 s | Detects silent peers and releases their slots within 40 s       |
| Concurrent connections         | 1024    | Bounds descriptors and tasks; the kernel backlog holds the rest     |

The first-request timeout covers what hyper's own header timer cannot: it runs
from the accepted connection (or the completed TLS handshake) until the first
request reaches the router, so a mute connection, a client that stops after the
HTTP/2 preface, and a slowloris connection all release their slot. It also
applies to each HTTP/1 keep-alive connection between requests, which means an
idle HTTP/1 connection closes after 10 s: a proxy that reuses connections to
Stargate must keep them for less. Traefik's
`serversTransport.forwardingTimeouts.idleConnTimeout` must stay below 10 s;
otherwise a request can start on a connection Stargate is closing, and a
non-idempotent request that Go does not retry can fail as `502`.

`LISTENER_<NAME>_MAX_CONNECTIONS` (`MAX_CONNECTIONS` for the implicit
listener) raises or lowers the cap per listener. A failed accept backs off for
100 ms before retrying, so an exhausted descriptor table cannot spin a core.
`TCP_NODELAY` is set on every accepted connection. An upgraded WebSocket leaves
the cap as soon as the upgrade completes: those connections are bounded by the
gateway's admission (`runtime` primary and mirror slots), not by the listener.

Each accepted connection carries the peer address and the listener's name,
planes, TLS state, and proxy trust; rules that differ per listener read them from
there instead of a process-wide setting.

## Startup and shutdown

All listeners bind before any of them serves, so a busy port stops startup
instead of leaving a half-open process. Each one logs its name, bound address,
planes, and TLS state.

On `SIGTERM` or `SIGINT` the process marks itself unready, waits
`SERVER_DRAIN_DELAY_SECS`, then stops accepting on every listener at once and
lets open connections finish within `SERVER_SHUTDOWN_TIMEOUT_SECS`. WebSocket
connections are closed with `1001` as described in `AGENTS.md`.

Because `PORT` does not apply with `LISTENERS`, the default issuer
(`http://localhost:5050`) is meaningless there: set `JWT_ISSUER` to the public URL.
Declaring `LISTENERS` without it stops startup; `OAUTH_BASE_URL` alone does not
set the issuer.
