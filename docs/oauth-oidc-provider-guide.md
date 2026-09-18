# OAuth/OIDC Provider Guide

Stargate can act as an OAuth 2.0 authorization server and OpenID Connect provider with admin-managed clients. Dynamic client registration is not supported.

## Endpoints

Discovery and keys:

- `GET /.well-known/oauth-authorization-server`
- `GET /.well-known/openid-configuration`
- `GET /.well-known/jwks.json`

OAuth/OIDC:

- `GET /oauth/authorize`
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

`JWT_SECRET` is used only with HMAC algorithms such as `HS256`.

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

First-party clients can skip consent when one of these attrs is true:

```json
{
  "first_party": true
}
```

Also accepted: `firstParty` and `trusted`.

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

If the user is not logged in, Stargate redirects back with:

```text
error=login_required
```

If a third-party client needs consent and no active consent exists:

```text
error=consent_required
```

Current first milestone has no browser consent UI. First-party clients can skip consent via client attrs.

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

Claims depend on granted scopes:

- `openid`: `sub`
- `email`: `email`, `email_verified`
- `profile`: `name`, `preferred_username`, `picture`

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
- Browser consent UI is not implemented.
- Authorization Code login UX is not implemented; unauthenticated requests redirect with `login_required`.
- Admin/user-facing consent management endpoints are not implemented.
- OAuth/OIDC error responses still use Stargate's generic error envelope for most non-redirect errors.
- OAuth AS metadata does not advertise `scopes_supported`; there is no global OAuth scope registry.
- OAuth clients cannot broadly introspect/revoke API keys; admin grants still handle API-key revocation.
- Full end-to-end OAuth/OIDC integration tests are still pending.
