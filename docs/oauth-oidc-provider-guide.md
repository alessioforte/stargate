# OAuth/OIDC Provider Guide

Stargate can act as an OAuth 2.0 authorization server and OpenID Connect provider with admin-managed clients. Dynamic client registration is not supported.

## Endpoints

Discovery and keys:

- `GET /.well-known/oauth-authorization-server`
- `GET /.well-known/openid-configuration`
- `GET /.well-known/jwks.json`

OAuth/OIDC:

- `GET /oauth/authorize`
- `POST /oauth/authorize` (form-encoded, same parameters)
- `POST /oauth/login` (no-JavaScript form post of the hosted login page; answers with redirects)
- `POST /oauth/token`
- `GET /oauth/userinfo`
- `POST /oauth/userinfo`
- `POST /oauth/introspect`
- `POST /oauth/revoke`

Admin client registry:

- `GET /admin/oauth/clients`
- `POST /admin/oauth/clients`
- `GET /admin/oauth/clients/{client_id}`
- `PUT /admin/oauth/clients/{client_id}`
- `PATCH /admin/oauth/clients/{client_id}`
- `PUT /admin/oauth/clients/{client_id}/disable`
- `PUT /admin/oauth/clients/{client_id}/enable`
- `PUT /admin/oauth/clients/{client_id}/rotate-secret`
- `DELETE /admin/oauth/clients/{client_id}`

## Configuration

Set a stable issuer and public base URL:

```env
JWT_ISSUER=https://auth.example.com
OAUTH_BASE_URL=https://auth.example.com
JWT_KID=stargate-current
JWKS_CACHE_MAX_AGE_SECS=300
JWT_ACCESS_EXP=15m
JWT_REFRESH_EXP=7d
```

Use asymmetric signing for production when external resource servers validate JWTs from JWKS:

```env
JWT_ALGORITHM=RS256
JWT_KID=stargate-current
```

`JWT_SECRET` is used only with HMAC algorithms such as `HS256`. HMAC secrets
are never published and cannot be rotated without invalidating every token.

OpenID Connect Discovery requires providers to support RS256 ID tokens, so keep
an RSA key active when third-party relying parties use Stargate. ES256/ES384
keys work, but the OpenID conformance suite flags a provider that only offers
them (Stargate logs a warning at startup).

### Signing keys and rotation

Asymmetric keys live under `.stargate/jwks/` (share this directory across
cluster nodes):

```text
.stargate/jwks/keys/<kid>/private.pem   signing key (needed only by the active key)
.stargate/jwks/keys/<kid>/public.pem    verification key
.stargate/jwks/keys/<kid>/alg           RS256, RS384, RS512, ES256, or ES384
```

`JWT_KID` names the active key, which signs every token. Every other directory
under `keys/` is a verification-only key: it is published in
`/.well-known/jwks.json` (active key first) and tokens carrying its `kid` keep
validating. Tokens are always verified with the key their `kid` names and that
key's algorithm; an unknown `kid` is rejected. On first start Stargate creates
`keys/<JWT_KID>/` with an `JWT_ALGORITHM` key (default RS256). `JWT_ALGORITHM`
may be left unset; when set it must match the active key, or startup fails.

Installations from before the keyring keep `.stargate/jwks/private.pem` and
`public.pem` (`es256/`, `es384/` for EC keys) as the active key until the first
rotation, which moves them into `keys/<current JWT_KID>/`.

Rotate without logging anyone out:

1. Create the next key: `stargate jwt generate-key --kid stargate-2026-11`
   (add `--algorithm ES256` to change algorithm). Restart every node: Stargate
   still signs with the current key and now also publishes the new one.
2. Wait until relying parties have refreshed their JWKS caches (at least
   `JWKS_CACHE_MAX_AGE_SECS`).
3. Set `JWT_KID=stargate-2026-11` (and `JWT_ALGORITHM` if it changed) and
   restart. New tokens use the new key; tokens signed by the old one stay valid.
4. After the longest token lifetime (`JWT_REFRESH_EXP` for native refresh
   tokens; OAuth refresh tokens are opaque and unaffected), delete
   `keys/<old kid>/` and restart. A retired key may drop its `private.pem`
   earlier: verification only needs `public.pem`.

For a compromised key, skip the waits: activate a new key and delete the
compromised directory at once. Every token it signed stops validating, so users
sign in again.

## Client Model

OAuth clients are stored in `oauth_clients`. Client secrets are returned only on create and rotate, then stored only as hashes.

Important fields:

- `token_endpoint_auth_method`: `client_secret_basic`, `client_secret_post`, or `none`
- `grant_types`: `client_credentials`, `authorization_code`, `refresh_token`
- `response_types`: `code`
- `redirect_uris`: exact redirect URIs, no wildcards
- `scopes`: allowed scopes
- `audiences`: allowed resource audiences
- `attrs`: optional client flags

First-party clients skip consent (see "Consent" below) when one of these
attrs is true:

```json
{
  "first_party": true
}
```

Also accepted: `firstParty` and `trusted`.

Confidential clients must send PKCE by default. OpenID Connect Core does not
require it, so a confidential client can opt out (public clients cannot):

```json
{
  "require_pkce": false
}
```

Also accepted: `requirePkce`. A code issued without a challenge is rejected at
the token endpoint if the request carries a `code_verifier`.

Privileged token operation clients can use:

```json
{
  "can_introspect": true,
  "can_revoke": true
}
```

Equivalent scopes also work:

- `oauth:introspect`
- `oauth:revoke`

## Create A Client

Confidential service client:

```bash
curl -X POST "$STARGATE/admin/oauth/clients" \
  -H "authorization: Bearer $ADMIN_ACCESS_TOKEN" \
  -H "content-type: application/json" \
  -d '{
    "clientId": "orders-worker",
    "name": "Orders Worker",
    "tokenEndpointAuthMethod": "client_secret_basic",
    "grantTypes": ["client_credentials"],
    "scopes": ["orders:read", "orders:write"],
    "audiences": ["orders-api"]
  }'
```

OIDC first-party web client:

```bash
curl -X POST "$STARGATE/admin/oauth/clients" \
  -H "authorization: Bearer $ADMIN_ACCESS_TOKEN" \
  -H "content-type: application/json" \
  -d '{
    "clientId": "web-app",
    "name": "Web App",
    "tokenEndpointAuthMethod": "client_secret_basic",
    "grantTypes": ["authorization_code", "refresh_token"],
    "responseTypes": ["code"],
    "redirectUris": ["https://app.example.com/oauth/callback"],
    "scopes": ["openid", "email", "profile", "offline_access"],
    "audiences": ["gateway"],
    "attrs": { "first_party": true }
  }'
```

Public SPA/native clients use:

```json
{
  "tokenEndpointAuthMethod": "none"
}
```

Public clients must use Authorization Code + PKCE. They cannot use `client_credentials`.

## Rotate Client Secret

```bash
curl -X PUT "$STARGATE/admin/oauth/clients/web-app/rotate-secret" \
  -H "authorization: Bearer $ADMIN_ACCESS_TOKEN"
```

The response includes the new secret once. Store it immediately in the client deployment secret store.

## Client Credentials

Request:

```bash
curl -X POST "$STARGATE/oauth/token" \
  -u "orders-worker:$CLIENT_SECRET" \
  -H "content-type: application/x-www-form-urlencoded" \
  -d "grant_type=client_credentials&scope=orders:read&audience=orders-api"
```

Response:

```json
{
  "access_token": "...",
  "token_type": "Bearer",
  "expires_in": 3600,
  "scope": "orders:read"
}
```

No refresh token is issued for `client_credentials`.

## Authorization Code + PKCE

Create a code verifier and S256 challenge in the client. Redirect the user:

```text
GET /oauth/authorize?
  response_type=code&
  client_id=web-app&
  redirect_uri=https%3A%2F%2Fapp.example.com%2Foauth%2Fcallback&
  scope=openid%20email%20profile%20offline_access&
  audience=gateway&
  state=opaque-state&
  nonce=opaque-nonce&
  code_challenge=BASE64URL_SHA256_VERIFIER&
  code_challenge_method=S256
```

If the user has no session, or `prompt=login` or an exceeded `max_age`
requires a fresh login, Stargate parks the request for 10 minutes and sends the
browser to the hosted auth app's login page:

```text
/auth/login?return_to=<issuer>/oauth/authorize?interaction=<id>&login_hint=...
```

The page offers the hosted app's full sign-in (password, email MFA, password
reset and change, organization choice) and prefills `login_hint`. After
sign-in it follows `return_to`, and `/oauth/authorize` resumes the parked
request. A session issued after the request was parked satisfies `prompt=login`
and `max_age`, so the user is not asked again.

The login page is prerendered at build time (`apps/auth/scripts/prerender.mjs`)
and hydrated in the browser, so it also works without JavaScript: its form then
posts natively to `POST /oauth/login`, which checks the password like
`POST /account/login` (login throttling, constant-time unknown users) and
redirects back to `/oauth/authorize`, or to the login page with `error=...`.
That path supports password-only sign-in: users who need MFA or a password
change get `error=javascript_required`. The parked request id reaches
`/oauth/login` in the `oauth_interaction` cookie (`HttpOnly`,
`SameSite=Strict`, `Path=/oauth/login`), which also blocks cross-site form
posts (login CSRF). The no-JavaScript path therefore needs the auth app on the
same site as Stargate, which is the default (`/auth`, served from
`AUTH_APP_DIR`).

`return_to` is only followed to in-app paths and to the auth app's own origin
or its API origin.

With `prompt=none` Stargate never shows the login page and redirects back with:

```text
error=login_required
```

The session cookie is `SameSite=Lax`, so a user already signed in to Stargate
is recognized when a relying party on another site redirects to
`/oauth/authorize`.

Errors that can be reported to a verified `redirect_uri` use the OAuth/OIDC
codes: `invalid_request`, `unsupported_response_type`, `invalid_scope`,
`unauthorized_client`, `login_required`, `interaction_required`,
`consent_required`, `request_not_supported`, and `request_uri_not_supported`.
Scopes the client is not registered for are ignored. Unknown parameters
(`display`, `ui_locales`, `acr_values`, ...) are ignored.

### Consent

First-party clients (see client attrs) never ask for consent. For other
clients, Stargate checks the user's stored consent: it must cover the
requested scopes and audience. If it does not, or the request has
`prompt=consent`, the request is parked again, bound to the signed-in user,
and the browser goes to the hosted consent page:

```text
/auth/consent?interaction=<id>
```

The page shows the application (name, description, the host it returns to,
and the `logo_uri`, `client_uri`, `policy_uri`, `tos_uri` client attrs when
they are HTTP(S) URLs), the signed-in account, and the requested scopes.
It uses two endpoints, both authenticated by the user's session cookie:

- `GET /oauth/interactions/{id}`: request details. Requests of other users
  answer `404 oauth_interaction.not_found`.
- `POST /oauth/interactions/{id}/consent` with `{ "approve": true|false }`:
  returns `{ "redirectTo": "..." }`. Approval stores the consent (merged with
  scopes granted before) and resumes the authorization request; denial ends
  it and redirects the client with `error=access_denied` and its `state`.

With `prompt=none` and no covering consent the client receives
`error=consent_required`. The consent page needs JavaScript.

Users manage their consents with:

- `GET /account/consents`: applications they allowed, with scopes.
- `DELETE /account/consents/{client_id}`: revoke (`204`, or `404
  oauth_consent.not_found`). The next authorization asks again, and the
  client's refresh tokens stop working (`invalid_grant`, the refresh family is
  dropped). Access tokens already issued stay valid until they expire.

Granting and revoking are audited in `outbox_events` as
`oauth_consent.granted`, `oauth_consent.updated` (a grant on top of an active
consent), and `oauth_consent.revoked`, with the user as actor.

Exchange code:

```bash
curl -X POST "$STARGATE/oauth/token" \
  -u "web-app:$CLIENT_SECRET" \
  -H "content-type: application/x-www-form-urlencoded" \
  -d "grant_type=authorization_code" \
  -d "code=$CODE" \
  -d "redirect_uri=https://app.example.com/oauth/callback" \
  -d "code_verifier=$CODE_VERIFIER"
```

Response:

```json
{
  "access_token": "...",
  "id_token": "...",
  "refresh_token": "...",
  "token_type": "Bearer",
  "expires_in": 3600,
  "scope": "openid email profile offline_access"
}
```

`refresh_token` appears only when `offline_access` was requested and the client allows `refresh_token`.

Token responses carry `Cache-Control: no-store`. An authorization code works
once; presenting it again fails with `invalid_grant` and revokes the access
token and refresh-token family issued for it.

## Refresh Token Grant

Refresh tokens are opaque. Stargate stores refresh-token family state in the `store` backend:

- Edge: memory store with periodic/on-shutdown backup
- Cluster: Redis

Request:

```bash
curl -X POST "$STARGATE/oauth/token" \
  -u "web-app:$CLIENT_SECRET" \
  -H "content-type: application/x-www-form-urlencoded" \
  -d "grant_type=refresh_token" \
  -d "refresh_token=$REFRESH_TOKEN"
```

Every successful use rotates the refresh token and returns a replacement. Stale-token reuse revokes the whole family.

## UserInfo

```bash
curl "$STARGATE/oauth/userinfo" \
  -H "authorization: Bearer $ACCESS_TOKEN"
```

`POST /oauth/userinfo` also accepts the token as a form-encoded
`access_token` body parameter (RFC 6750 §2.2).

Claims depend on granted scopes:

- `openid`: `sub`
- `email`: `email`, `email_verified`
- `profile`: `name`, `given_name`, `family_name`, `nickname`, `preferred_username`, `picture`, `updated_at`

Client credentials tokens are rejected by UserInfo.

## JWKS Validation

Resource servers should:

1. Fetch `/.well-known/openid-configuration`.
2. Read `jwks_uri`.
3. Cache JWKS according to `Cache-Control`.
4. Verify JWT signature and `kid`.
5. Check `iss`.
6. Check `aud` for the protected resource.
7. Check `exp`, `iat`, `typ`, and scopes.

Stargate gateway routes can perform these checks and resolve the access token
back to its active user session with an audience-bound auth policy:

```yaml
http:
  policies:
    app-user-auth:
      kind: auth
      strategies: [oauth]
      audience: gateway
```

Attach the policy before any `access_control` policy on the route. The OAuth
client must list `gateway` in `audiences`, and the authorization request must
request `audience=gateway`. The gateway accepts Authorization Code user access
tokens only: the token's `sid`, `sub_id`, and `azp` must resolve to an active
session linked to an enabled OAuth client.

JWT access-token claims include:

- `iss`
- `sub`
- `aud`
- `azp`
- `scope`
- `typ = bearer`
- `jti`
- `iat`
- `exp`

OIDC ID-token claims include:

- `iss`
- `sub`
- `aud`
- `azp`
- `iat`
- `exp`
- `auth_time`
- `nonce`
- scope-filtered profile/email claims

## Introspection

Admin callers can use existing admin grants. OAuth clients can authenticate with `client_secret_basic` or `client_secret_post`.

Own-token introspection:

```bash
curl -X POST "$STARGATE/oauth/introspect" \
  -u "orders-worker:$CLIENT_SECRET" \
  -H "content-type: application/x-www-form-urlencoded" \
  -d "token=$ACCESS_TOKEN"
```

Broader introspection requires:

- `oauth:introspect` scope or `attrs.can_introspect = true`
- token audience present in the client's registered audiences

Invalid, inactive, or invisible tokens return:

```json
{
  "active": false
}
```

## Revocation

```bash
curl -X POST "$STARGATE/oauth/revoke" \
  -u "orders-worker:$CLIENT_SECRET" \
  -H "content-type: application/x-www-form-urlencoded" \
  -d "token=$ACCESS_TOKEN"
```

Broader revocation requires:

- `oauth:revoke` scope or `attrs.can_revoke = true`
- token audience present in the client's registered audiences

JWT revocation stores revoked `jti` state until token expiry. Invisible or inactive tokens are no-op success.

## Current Limits

- Dynamic client registration is not supported.
- `oauth_client_secrets` history/overlap is not implemented; rotation replaces the active hash.
- Without JavaScript, the hosted login page signs in with a password only; MFA and password changes need JavaScript. The consent page always needs JavaScript.
- Request objects (`request`, `request_uri`) and the `claims` parameter are not supported.
- There is no UI yet for users to review/revoke consents (the `/account/consents` API exists) and no admin consent endpoints.
- OAuth/OIDC error responses still use Stargate's generic error envelope for most non-redirect errors.
- OAuth AS metadata does not advertise `scopes_supported`; there is no global OAuth scope registry.
- OAuth clients cannot broadly introspect/revoke API keys; admin grants still handle API-key revocation.
- Full end-to-end OAuth/OIDC integration tests are still pending.
