# OpenID Connect conformance

Stargate's OpenID Provider is tested in CI with the
[OpenID Foundation conformance suite](https://gitlab.com/openid/conformance-suite),
run locally inside the job. Local results cannot be submitted for
certification; certification requires the hosted suite at
`www.certification.openid.net` against a publicly reachable deployment.

## What runs in CI

[`.github/workflows/oidc-conformance.yml`](../.github/workflows/oidc-conformance.yml)
runs on pushes and pull requests that touch the OIDC/JWT code, and on demand:

1. Starts the prebuilt suite images (`SUITE_VERSION`) with Docker Compose.
2. Builds the `edge` profile and runs it with TLS on `https://stargate.local:5443`,
   a name the suite container resolves to the runner.
3. Runs `oidcc-config-certification-test-plan` (the **Config OP** profile) with
   [`.github/oidc-conformance/config-op.json`](../.github/oidc-conformance/config-op.json).
   It validates the discovery document against OpenID Connect Discovery and
   RFC 8414, the JWKS, and HTTPS on every endpoint.
4. Fails on any unexpected failure or warning, and uploads the exported plan
   results and server logs as the `oidc-conformance-results` artifact.

Current result: Config OP passes with no failures or warnings.

## Run it locally

```bash
# 1. Conformance suite (any directory outside the checkout)
git clone --depth 1 --branch release-v5.3.2 https://gitlab.com/openid/conformance-suite.git
IMAGE_TAG=release-v5.3.2 docker compose --project-name oidc-conformance \
  -f conformance-suite/docker-compose-prebuilt.yml \
  -f <stargate>/.github/oidc-conformance/compose.override.yml up -d

# 2. Stargate with TLS, from a scratch directory
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj "/CN=stargate.local" \
  -addext "subjectAltName=DNS:stargate.local" -keyout key.pem -out cert.pem
TLS_ENABLED=true TLS_CERT="$PWD/cert.pem" TLS_KEY="$PWD/key.pem" PORT=5443 \
  JWT_ISSUER=https://stargate.local:5443 <stargate>/target/debug/stargate

# 3. Runner
python3 -m venv venv && venv/bin/pip install -r conformance-suite/scripts/requirements.txt
CONFORMANCE_SERVER=https://localhost.emobix.co.uk:8443/ CONFORMANCE_DEV_MODE=1 \
  venv/bin/python conformance-suite/scripts/run-test-plan.py \
  oidcc-config-certification-test-plan <stargate>/.github/oidc-conformance/config-op.json
```

The suite UI is at `https://localhost.emobix.co.uk:8443` (self-signed
certificate). Stop it with
`docker compose --project-name oidc-conformance down`.

## Gaps before the Basic OP plan

The **Basic OP** profile (`oidcc-basic-certification-test-plan`) drives full
Authorization Code logins through a headless browser. Stargate cannot run it
yet. These gaps were found while wiring the suite and are ordered by impact:

1. **The authorization endpoint never authenticates the user.** Without a
   session, `/oauth/authorize` redirects back to the client with
   `error=login_required` for every request, not only for `prompt=none`. OIDC
   Core 3.1.2.3 expects the provider to show its login and resume the request.
   Today each relying party has to send users to `/auth/login?return_to=...`
   itself, which is how the Console works. `prompt=login` and an exceeded
   `max_age` also return `login_required` instead of re-authenticating.
2. **The session cookie is `SameSite=Strict`.** A browser does not send it
   when another site redirects to `/oauth/authorize`, so even signed-in users
   look anonymous to third-party relying parties.
3. **PKCE is required for every client.** The Basic plan sends PKCE in only one
   test; without it Stargate returns `400` JSON before redirecting. OAuth 2.1
   recommends PKCE for all clients, but OIDC Core does not require it for
   confidential clients, so certification needs an opt-out per client.
4. **Consent only works for first-party clients.** There is no consent UI;
   other clients get `consent_required` (see the provider guide). The suite's
   client can be registered as first-party, so this does not block CI.
5. **Error codes and unsupported parameters.** `unsupported_response_type` and
   `invalid_scope` are reported as `invalid_request`; `request`, `request_uri`,
   and `claims` are silently ignored instead of returning
   `request_not_supported` / `request_uri_not_supported`.
6. **Token responses lack `Cache-Control: no-store`** (RFC 6749 §5.1).
7. **Browser automation.** The hosted login is a JavaScript SPA whose inputs
   have no stable `id`/`name`. The suite's HtmlUnit browser needs either a
   server-rendered login page or stable selectors; `autocomplete="username"`,
   `type="password"`, and `type="submit"` are the closest today.

Once items 1–3 are addressed, add the Basic plan to the workflow with a static
first-party client registered through `POST /admin/oauth/clients` (redirect URI
`https://localhost.emobix.co.uk:8443/test/a/stargate/callback`), browser
automation steps for the login page, and an expected-failures file for anything
intentionally unsupported.
