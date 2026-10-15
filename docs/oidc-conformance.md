# OpenID Connect conformance

Stargate's OpenID Provider is tested in CI with the
[OpenID Foundation conformance suite](https://gitlab.com/openid/conformance-suite),
run locally inside the job. Local results cannot be submitted for
certification; certification requires the hosted suite at
`www.certification.openid.net` against a publicly reachable deployment.

## What runs in CI

[`.github/workflows/oidc-conformance.yml`](../.github/workflows/oidc-conformance.yml)
runs on pushes and pull requests that touch the OIDC/JWT/login code, and on
demand:

1. Starts the prebuilt suite images (`SUITE_VERSION`) with Docker Compose.
2. Builds the `edge` profile and the hosted auth app (`apps/auth`), bootstraps a
   test user, and runs Stargate with TLS on `https://stargate.local:5443`, a
   name the suite container resolves to the runner. It seeds
   [`stargate-config.yaml`](../.github/oidc-conformance/stargate-config.yaml),
   which relaxes the per-IP rate limit (the suite sends everything from one IP),
   and sets `MFA_MODE=off` so the suite's browser signs in with a password.
3. Runs `oidcc-config-certification-test-plan` (the **Config OP** profile) with
   [`config-op.json`](../.github/oidc-conformance/config-op.json): the discovery
   document against OpenID Connect Discovery and RFC 8414, the JWKS, and HTTPS
   on every endpoint.
4. Runs `oidcc-basic-certification-test-plan` (the **Basic OP** profile, static
   client, discovery). [`basic-op-setup.sh`](../.github/oidc-conformance/basic-op-setup.sh)
   registers three first-party confidential clients (`client`, `client2`, and a
   `client_secret_post` one) and writes the plan config, including the browser
   steps that fill the hosted login page (`/auth/login`). The suite's HtmlUnit
   browser cannot run the app's ES module bundle, so it uses the page's
   prerendered form, which posts natively to `/oauth/login`.
5. Fails on any unexpected failure, warning, or skip, and uploads the exported
   plan results and server logs as the `oidc-conformance-results` artifact.

Current results:

- Config OP: passes with no failures or warnings.
- Basic OP: 36 modules, no failures. Four tests end in `REVIEW` because
  certification needs a screenshot: `oidcc-prompt-login` and `oidcc-max-age-1`
  (the login page shown again) and `oidcc-ensure-registered-redirect-uri` and
  `oidcc-ensure-request-object-with-redirect-uri` (the error page for an
  unregistered `redirect_uri`). The expected warnings and skips are listed with
  their reasons in
  [`basic-op-expected-failures.json`](../.github/oidc-conformance/basic-op-expected-failures.json)
  and [`basic-op-expected-skips.json`](../.github/oidc-conformance/basic-op-expected-skips.json):
  - Scope-granted email/profile claims are also in the ID token, not only in
    UserInfo.
  - The profile claims Stargate does not store (`website`, `gender`,
    `birthdate`, `zoneinfo`, `locale`, ...) are missing.
  - `acr_values` and the `claims` parameter are ignored.
  - The `address` and `phone` scopes and request objects are not supported.

## Run it locally

```bash
# 1. Conformance suite (any directory outside the checkout)
git clone --depth 1 --branch release-v5.3.2 https://gitlab.com/openid/conformance-suite.git
IMAGE_TAG=release-v5.3.2 docker compose --project-name oidc-conformance \
  -f conformance-suite/docker-compose-prebuilt.yml \
  -f <stargate>/.github/oidc-conformance/compose.override.yml up -d

# 2. Auth app (prerendered login page), built into <stargate>/.stargate/apps/auth
(cd <stargate>/apps/auth && npm ci && VITE_API_URL= npm run build)

# 3. Stargate with TLS, from a scratch directory
mkdir -p .stargate && cp <stargate>/.github/oidc-conformance/stargate-config.yaml .stargate/config.yaml
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj "/CN=stargate.local" \
  -addext "subjectAltName=DNS:stargate.local" -keyout key.pem -out cert.pem
export TLS_ENABLED=true TLS_CERT="$PWD/cert.pem" TLS_KEY="$PWD/key.pem" PORT=5443 \
  JWT_ISSUER=https://stargate.local:5443 MFA_MODE=off \
  AUTH_APP_DIR=<stargate>/.stargate/apps/auth
<stargate>/target/debug/stargate admin bootstrap --email oidc-tester@example.com \
  --password 'Blue-Harbor-7319!' --name "Oidc Tester" --nickname tester
<stargate>/target/debug/stargate

# 4. Runner
python3 -m venv venv && venv/bin/pip install -r conformance-suite/scripts/requirements.txt
export CONFORMANCE_SERVER=https://localhost.emobix.co.uk:8443/ CONFORMANCE_DEV_MODE=1
venv/bin/python conformance-suite/scripts/run-test-plan.py \
  oidcc-config-certification-test-plan <stargate>/.github/oidc-conformance/config-op.json

API_URL=https://127.0.0.1:5443 <stargate>/.github/oidc-conformance/basic-op-setup.sh \
  https://stargate.local:5443 oidc-tester@example.com 'Blue-Harbor-7319!' basic-op.json
venv/bin/python conformance-suite/scripts/run-test-plan.py \
  --expected-failures-file <stargate>/.github/oidc-conformance/basic-op-expected-failures.json \
  --expected-skips-file <stargate>/.github/oidc-conformance/basic-op-expected-skips.json \
  "oidcc-basic-certification-test-plan[server_metadata=discovery][client_registration=static_client]" \
  basic-op.json
```

The suite UI is at `https://localhost.emobix.co.uk:8443` (self-signed
certificate). Stop it with
`docker compose --project-name oidc-conformance down`.

## Certification

Local results cannot be submitted. To certify Basic OP, run the same plan on
`www.certification.openid.net` against a publicly reachable deployment, upload
screenshots for the four `REVIEW` tests, and submit the plan. Warnings do not
block certification, but they are visible in the published results.
