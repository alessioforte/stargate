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
- `GET /oauth/logout`, `POST /oauth/logout` (`end_session_endpoint`, see "Logout")
- `POST /oauth/logout/confirm` (form post of the hosted logout page)

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

OpenID Connect requires providers to support RS256 ID tokens (Discovery §3,
Core §15.1). ID tokens are signed with the active key unless
`JWT_ID_TOKEN_KID` names a separate RS256 key. Set it when `JWT_KID` is an EC
key or when sessions use an HMAC secret:

```env
JWT_ALGORITHM=ES256
JWT_KID=ec-2026-10
JWT_ID_TOKEN_KID=rsa-2026-10
```

Access, session, and refresh tokens then use the active key, and ID tokens use
the RS256 key; discovery advertises RS256 and both keys are in the JWKS. With an
HMAC secret only the RS256 key is published. Without `JWT_ID_TOKEN_KID`, a
non-RS256 active key makes Stargate log a warning and fails the OpenID Config
OP discovery check.

### Signing keys and rotation

Asymmetric keys live under `.stargate/jwks/` (share this directory across
cluster nodes):

```text
.stargate/jwks/keys/<kid>/private.pem   signing key (needed by the active and ID token keys)
.stargate/jwks/keys/<kid>/public.pem    verification key
.stargate/jwks/keys/<kid>/alg           RS256, RS384, RS512, ES256, or ES384
```

`JWT_KID` names the active key, which signs every token. Every other directory
under `keys/` is a verification-only key: it is published in
`/.well-known/jwks.json` (active key first) and tokens carrying its `kid` keep
validating. Tokens are always verified with the key their `kid` names and that
key's algorithm; an unknown `kid` is rejected. On first start Stargate creates
`keys/<JWT_KID>/` with an `JWT_ALGORITHM` key (default RS256), and
`keys/<JWT_ID_TOKEN_KID>/` (RS256) when set. Keys are generated only on a fresh
keyring: once other keys exist, a `JWT_KID` or `JWT_ID_TOKEN_KID` naming a
missing key fails startup instead of silently minting an unpublished key.
`JWT_ALGORITHM` may be left unset; when set it must match the active key, or
startup fails.

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

The ID token key rotates the same way, independently: `stargate jwt
generate-key --kid rsa-2026-11 --id-token`, restart, wait, set
`JWT_ID_TOKEN_KID`, restart, and delete the old directory after the ID token
lifetime (`JWT_ACCESS_EXP`).

For a compromised key, skip the waits: activate a new key and delete the
compromised directory at once. Every token it signed stops validating, so users
sign in again.

On Kubernetes, the Helm chart mounts one Secret per key at `keys/<kid>/` and
sets `JWT_KID` / `JWT_ID_TOKEN_KID` from `jwt.activeKid` / `jwt.idTokenKid`;
each step above is a `helm upgrade`. See the
[chart README](../k8s/stargate/README.md#jwt-signing-keys).

## Client Model

OAuth clients are stored in `oauth_clients`. Client secrets are returned only on create and rotate, then stored only as hashes.

Important fields:

- `token_endpoint_auth_method`: `client_secret_basic`, `client_secret_post`,
  `private_key_jwt`, or `none` (see "Client Authentication")
- `jwks` / `jwks_uri`: the public keys of a `private_key_jwt` client, inline or
  at an HTTPS URL; exactly one, and only for that method
- `grant_types`: `client_credentials`, `authorization_code`, `refresh_token`
- `response_types`: `code`
- `redirect_uris`: exact redirect URIs, no wildcards
- `post_logout_redirect_uris`: where RP-Initiated Logout may return the browser
  (exact match, same rules as `redirect_uris`)
- `backchannel_logout_uri`: absolute HTTPS URL (HTTP only on loopback hosts),
  no fragment, that receives Back-Channel Logout tokens
- `backchannel_logout_session_required`: the client needs `sid` in logout
  tokens; Stargate always sends it
- `scopes`: allowed scopes
- `audiences`: allowed resource audiences
- `attrs`: optional client flags

First-party clients skip consent (see "Consent" below) with:

```json
{
  "first_party": true
}
```

Confidential clients must send PKCE by default. OpenID Connect Core does not
require it, so a confidential client can opt out (public clients cannot):

```json
{
  "require_pkce": false
}
```

A code issued without a challenge is rejected at the token endpoint if the
request carries a `code_verifier`.

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

A client that signs its own assertions registers its public keys instead of
receiving a secret:

```json
{
  "tokenEndpointAuthMethod": "private_key_jwt",
  "jwksUri": "https://app.example.com/.well-known/jwks.json"
}
```

or `"jwks": {"keys": [...]}` with the keys inline. Inline sets must hold public
RSA or EC (P-256, P-384) keys only: private members (`d`, ...) and symmetric
keys are rejected. `jwksUri` is an absolute HTTPS URL without a fragment
(HTTP only on loopback hosts). Changing the method to `private_key_jwt` clears
the client's secret; a `PATCH` that sets one of `jwks` / `jwksUri` clears the
other.

## Rotate Client Secret

```bash
curl -X PUT "$STARGATE/admin/oauth/clients/web-app/rotate-secret" \
  -H "authorization: Bearer $ADMIN_ACCESS_TOKEN"
```

The response includes the new secret once. Store it immediately in the client deployment secret store.

## Client Authentication

The token, introspection, and revocation endpoints accept one authentication
method per request; a request that combines them (for example HTTP Basic and a
`client_assertion`) fails with `invalid_request`. A client can only use the
method it is registered with.

- `client_secret_basic`: HTTP Basic with the client id and secret.
- `client_secret_post`: `client_id` and `client_secret` form fields.
- `none`: only `client_id` (public clients).
- `private_key_jwt` (OpenID Connect Core §9, RFC 7523): the client signs a JWT
  with one of its registered keys and sends it with
  `client_assertion_type=urn:ietf:params:oauth:client-assertion-type:jwt-bearer`
  and `client_assertion=<JWT>` (`client_id` is optional and must match).

```bash
curl -X POST "$STARGATE/oauth/token" \
  -H "content-type: application/x-www-form-urlencoded" \
  -d "grant_type=client_credentials&scope=orders:read" \
  -d "client_assertion_type=urn:ietf:params:oauth:client-assertion-type:jwt-bearer" \
  -d "client_assertion=$ASSERTION"
```

The assertion must:

- be signed with `RS256`/`384`/`512`, `PS256`/`384`/`512`, `ES256`, or `ES384`
  (discovery: `token_endpoint_auth_signing_alg_values_supported`); with a
  `kid`, the registered key with that `kid` is used, else every key that fits
  the algorithm is tried;
- have `iss` and `sub` equal to the client id;
- have an `aud` (string or array) containing the issuer, the token endpoint
  URL, or the URL of the endpoint called (`/oauth/introspect`,
  `/oauth/revoke`);
- carry `exp` no more than an hour ahead (60 seconds of clock skew are
  tolerated), and `nbf` when present must have passed;
- carry a `jti`. Each `jti` authenticates once: Stargate remembers it until
  the assertion expires, so a replay fails with `invalid_client`.

Keys at `jwks_uri` are fetched with a 5-second timeout (no redirects, at most
64 KiB) and cached for 5 minutes per URL. When no cached key verifies an
assertion, Stargate refetches the document, at most once every 10 seconds per
URL, so a client rotates by publishing the new key before signing with it.
Clients behind a private CA can be trusted with `OAUTH_CLIENT_CA_FILE` (PEM
bundle, added to the system roots), which also covers back-channel logout.

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

`id_token_hint` names the user the client expects: an ID token Stargate issued
(possibly expired). A hint that does not verify (signature, issuer) fails with
`invalid_request`. When the signed-in user is someone else, `prompt=none`, or a
request resumed after sign-in, fails with `login_required`; otherwise the
browser goes to the login page prefilled with the hinted user's email (unless
the request has its own `login_hint`).

The session cookie is `SameSite=Lax`, so a user already signed in to Stargate
is recognized when a relying party on another site redirects to
`/oauth/authorize`.

`response_mode` picks how the response reaches `redirect_uri`:

- `query` (default): a `302` with `code` and `state` (or the error) in the
  query.
- `form_post` (OAuth 2.0 Form Post Response Mode): an HTML page whose form
  posts the same parameters to `redirect_uri` as soon as it loads (a button
  without JavaScript). The page is `Cache-Control: no-store` and sends no
  referrer, and the code never appears in a URL.

Any other mode (`fragment`, `jwt`, ...) fails with `invalid_request`, reported
in the query. Errors, including a denied consent, use the requested mode. A
request object is rejected (`request_not_supported`) in the `response_mode`
and with the `state` it carries, read without verifying it; the
`redirect_uri` is the verified one from the query.

Errors that can be reported to a verified `redirect_uri` use the OAuth/OIDC
codes: `invalid_request`, `unsupported_response_type`, `invalid_scope`,
`unauthorized_client`, `login_required`, `interaction_required`,
`consent_required`, `request_not_supported`, and `request_uri_not_supported`.
Scopes the client is not registered for are ignored. Unknown parameters
(`display`, `ui_locales`, ...) are ignored. `acr_values` is described in
"Authentication context" below.

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

### Authentication context (`amr`, `acr`)

Every login session records how the user signed in, as OpenID Connect `amr`
values (RFC 8176):

| Sign-in | `amr` | `acr` |
|---|---|---|
| Password (hosted login, `/oauth/login`, `/account/login`) | `["pwd"]` | `urn:stargate:acr:single-factor` |
| Password and email code (login MFA) | `["pwd", "otp", "mfa"]` | `urn:stargate:acr:multi-factor` |
| Email code only (passwordless) | `["otp"]` | `urn:stargate:acr:single-factor` |
| Google or GitHub | `["fed"]` | `urn:stargate:acr:single-factor` |

`fed` is not registered by RFC 8176; other providers use it for federated
sign-in. An email code verified later in the session (`POST
/account/mfa/challenges`, any purpose) adds `otp`, and `mfa` after a password
or an external provider. The level stays for the session's lifetime;
switching organization keeps it.

ID tokens and OAuth access tokens carry `acr` and `amr`, introspection
returns them with `auth_time` (RFC 9470), and tokens from a refresh keep the
values of the original grant. Discovery advertises both levels in
`acr_values_supported`.

A client asks for a level with `acr_values` (space-separated, most preferred
first; unknown values are ignored). Stargate issues the highest requested
level the session reaches. When every requested level is above it (a
single-factor session asked for `urn:stargate:acr:multi-factor`):

- the request is parked and the browser goes to the hosted step-up page
  (`/auth/step-up?interaction=...`, JavaScript required). The user enters an
  email code there; the session gains `otp` and `mfa` and the request resumes;
- an email-code-only session cannot gain a second factor from another email
  code, so the user signs in with a password first, then steps up. If they
  sign in with an email code again, the actual level is issued instead of
  looping;
- with `prompt=none` the client gets `interaction_required`;
- when MFA is unavailable (`MFA_MODE=off`, or email is not one of the user's
  MFA methods) the session's actual level is issued: `acr_values` is
  voluntary, so the client decides whether it is enough.

Admin session listings (`GET /admin/sessions`) show each session's
`authenticationMethods`.

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

Admin callers can use existing admin grants. OAuth clients authenticate as at the token endpoint (`client_secret_basic`, `client_secret_post`, or `private_key_jwt`; see "Client Authentication").

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

## Logout

### RP-Initiated Logout

Discovery advertises `end_session_endpoint` (`/oauth/logout`, OpenID Connect
RP-Initiated Logout 1.0). Send the browser there with:

```text
GET /oauth/logout?
  id_token_hint=<ID token issued to the client>&
  post_logout_redirect_uri=https%3A%2F%2Fapp.example.com%2Flogged-out&
  state=opaque-state
```

- `id_token_hint` is recommended and may be expired. It must verify, and
  `client_id`, when also sent, must equal its audience.
- `post_logout_redirect_uri` must exactly match one of the client's
  `post_logout_redirect_uris`; the client comes from `client_id` or the hint.
  `state` is returned unchanged. Without it Stargate shows the auth app's
  signed-out page (`/auth/signed-out`).
- `logout_hint` and `ui_locales` are accepted and ignored.
- `POST` takes the same form-encoded parameters. A cross-site form post
  arrives without the `SameSite=Lax` session cookie, so it is redirected to
  the `GET` form first.

When the hint names the browser's current session (same user and `sid`), the
session ends at once. Otherwise, with no hint, a hint for another user, or a
hint for an older session, Stargate parks the request for 10 minutes and the
user confirms on the auth app's logout page (`/auth/logout`), so a link on
another site cannot sign users out. The page is prerendered and its form posts
natively to `/oauth/logout/confirm`, with or without JavaScript; the parked
request travels in the `oauth_logout` cookie (`HttpOnly`, `SameSite=Strict`,
`Path=/oauth/logout`). "Stay signed in" returns to the client (or the auth
app) without signing out. A browser without a session is sent straight to
the destination.

An invalid hint, an unregistered or client-less `post_logout_redirect_uri`, or
an unknown `client_id` answers `400 invalid_request` and never redirects, and
the session stays.

Ending the session deletes the browser's login session (`sid`) and clears the
session cookie. Every OAuth token linked to that session stops working for
Stargate's own checks (userinfo, introspection, refresh), and relying parties
linked to it receive back-channel logout tokens.

### Back-Channel Logout

Discovery advertises `backchannel_logout_supported` and
`backchannel_logout_session_supported` (OpenID Connect Back-Channel Logout
1.0). Clients that registered a `backchannel_logout_uri` and received tokens in
a session get a `POST` with a form-encoded `logout_token` whenever that session
ends:

- RP-Initiated Logout and `DELETE /account/logout`;
- `POST /account/logout/all` and the revocation of a native refresh token;
- admin session revocation (one session or all of a user's) and user deletion;
- removal from an organization, or its deletion, for the sessions acting in it.

A session that simply expires is not announced; clients rely on token expiry.

The logout token is a JWT signed with the ID token key (`typ: logout+jwt`,
RS256 when `JWT_ID_TOKEN_KID` is set) with `iss`, `sub` (the ID token
subject), `aud` (the client), `iat`, `exp` (two minutes), `jti`, `sid`, and
`events: {"http://schemas.openid.net/event/backchannel-logout": {}}`, and no
`nonce`. Verify it with the JWKS like an ID token.

Delivery runs in the Stargate process, best effort: a 5-second timeout, two
retries (after 1 and 3 seconds) on network errors and `5xx`, none on `4xx`,
redirects not followed. RP-Initiated Logout waits up to 3 seconds for delivery
before redirecting the browser. Nothing is persisted, so notifications in
flight during a restart are lost. Relying parties behind a private CA can be
trusted with `OAUTH_CLIENT_CA_FILE` (PEM bundle, added to the system
roots).

The Console logs out this way: it sends its ID token to `/oauth/logout` and
returns to its root, its registered `post_logout_redirect_uri` (`admin
bootstrap` registers the callback URI without `/auth/callback`).

## Current Limits

- Dynamic client registration is not supported.
- `oauth_client_secrets` history/overlap is not implemented; rotation replaces the active hash.
- Without JavaScript, the hosted login page signs in with a password only; MFA and password changes need JavaScript. The consent page always needs JavaScript.
- Request objects (`request`, `request_uri`) and the `claims` parameter are not supported.
- `response_mode` is `query` or `form_post`: `fragment` (no implicit or hybrid flows) and JARM (`jwt`) are not supported.
- Client authentication with `client_secret_jwt` or mutual TLS (`tls_client_auth`, `self_signed_tls_client_auth`) is not supported.
- There is no UI yet for users to review/revoke consents (the `/account/consents` API exists) and no admin consent endpoints.
- OAuth/OIDC error responses still use Stargate's generic error envelope for most non-redirect errors.
- OAuth AS metadata does not advertise `scopes_supported`; there is no global OAuth scope registry.
- OAuth clients cannot broadly introspect/revoke API keys; admin grants still handle API-key revocation.
- Full end-to-end OAuth/OIDC integration tests are still pending.
- Front-Channel Logout and OIDC Session Management (`check_session_iframe`) are not supported.
- `acr` can only be requested with `acr_values`: the `claims` parameter (essential `acr`) is not supported. The step-up page needs JavaScript and offers email codes only.
- Back-channel logout delivery is in-process and not persisted (see "Back-Channel Logout").
