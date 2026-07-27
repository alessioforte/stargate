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
VITE_API_URL=http://localhost:5050
VITE_AUTH_URL=http://localhost:3010
VITE_OAUTH_CLIENT_ID=stargate_admin
VITE_OAUTH_REDIRECT_URI=http://localhost:3011/auth/callback
```

Provision the client together with the first administrator:

```bash
cargo run --features edge -- admin bootstrap \
  --email admin@localhost \
  --generate-password \
  --oauth-redirect-uri http://localhost:3011/auth/callback
```

The resulting `stargate_admin` OAuth client is a public browser client:

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
    "trusted": true,
    "system": true
  }
}
```

No client secret is used because this is a browser app. PKCE protects the
authorization code flow.

When opening the admin app from another device, serve it over HTTPS. Browsers
expose `crypto.subtle` only in secure contexts, and the PKCE flow needs
SHA-256. `http://localhost` works for local development, but
`http://<raspberry-pi-ip>` is not treated as secure. For a device-accessible
build, point `VITE_API_URL`, `VITE_AUTH_URL`, and `VITE_OAUTH_REDIRECT_URI` at
the HTTPS Pi hostname, or omit the URL values to use the current origin.

## Checks

```bash
npm run build
npm run lint
```
