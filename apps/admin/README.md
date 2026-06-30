# Stargate Admin

Vite React admin app for Stargate. Authentication is handled through the hosted
Stargate auth app with OAuth Authorization Code + PKCE.

## Local Development

```bash
npm install
npm run dev
```

The dev server listens on `http://localhost:3011`.

Required local env:

```text
PUBLIC_RUNTIME_API_URL=http://localhost:5050
PUBLIC_RUNTIME_AUTH_URL=http://localhost:3010
PUBLIC_RUNTIME_OAUTH_CLIENT_ID=stargate_admin
PUBLIC_RUNTIME_OAUTH_REDIRECT_URI=http://localhost:3011/auth/callback
```

The `stargate_admin` OAuth client should be a public browser client:

```json
{
  "clientId": "stargate_admin",
  "tokenEndpointAuthMethod": "none",
  "grantTypes": ["authorization_code", "refresh_token"],
  "responseTypes": ["code"],
  "redirectUris": ["http://localhost:3011/auth/callback"],
  "scopes": ["openid", "email", "profile", "offline_access"],
  "attrs": {
    "first_party": true,
    "trusted": true
  }
}
```

No client secret is used because this is a browser app. PKCE protects the
authorization code flow.

## Checks

```bash
npm run build
npm run lint
```
