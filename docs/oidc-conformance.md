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

1. Creates a test CA and a certificate it signs for the suite's hostname
   (`localhost.emobix.co.uk`), and starts the prebuilt suite images
   (`SUITE_VERSION`) with Docker Compose;
   [`compose.override.yml`](../.github/oidc-conformance/compose.override.yml)
   serves the suite with that certificate (from `SUITE_TLS_DIR`), so Stargate
   can verify it when it delivers back-channel logout tokens
   (`BACKCHANNEL_LOGOUT_CA_FILE`).
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
   client, discovery). [`op-setup.sh`](../.github/oidc-conformance/op-setup.sh)
   registers three first-party confidential clients (`client`, `client2`, and a
   `client_secret_post` one) with the suite's `post_logout_redirect` URL, and
   writes `op.json`, including the browser steps for the hosted login page
   (`/auth/login`) and the logout pages. The suite's HtmlUnit browser cannot
   run the app's ES module bundle, so it uses the pages' prerendered forms,
   which post natively to `/oauth/login` and `/oauth/logout/confirm`.
5. Runs `oidcc-rp-initiated-logout-certification-test-plan` (**RP-Initiated
   Logout OP**) with `op.json`, and
   `oidcc-backchannel-rp-initiated-logout-certification-test-plan`
   (**Back-Channel Logout OP**) with `backchannel-op.json`: the same config
   with a fourth client that also registers the suite's `backchannel_logout`
   URL. The other clients must not register it: Stargate notifies every
   linked client that has one when a session ends, and the RP-Initiated Logout
   tests fail on any request to a suite URL they do not serve
   (`UnexpectedHttpRequestReceived`).
6. Fails on any unexpected failure, warning, or skip, and uploads the exported
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
- RP-Initiated Logout OP: 12 modules, no failures or warnings. Eight tests end
  in `REVIEW` for screenshots, which the browser steps capture: the
  signed-out page (`no-params` and `only-state`, after confirming on the
  logout page, and `no-post-logout-redirect-uri`) and the `400` error page for
  a missing, modified, or foreign-signed `id_token_hint` with a
  `post_logout_redirect_uri`, or an unregistered one.
- Back-Channel Logout OP: no failures or warnings; the suite validates the
  logout token's signature and claims against the ID token.

## Run it locally

```bash
# 1. Conformance suite (any directory outside the checkout), served with a
#    certificate from a test CA that Stargate trusts for back-channel logout
git clone --depth 1 --branch release-v5.3.2 https://gitlab.com/openid/conformance-suite.git
export SUITE_TLS_DIR="$PWD/suite-tls" && mkdir -p "$SUITE_TLS_DIR" && (cd "$SUITE_TLS_DIR" &&
  openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj "/CN=Conformance test CA" \
    -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign" \
    -keyout ca.key -out ca.crt &&
  openssl req -newkey rsa:2048 -nodes -subj "/CN=localhost.emobix.co.uk" -keyout suite.key -out suite.csr &&
  printf 'subjectAltName=DNS:localhost.emobix.co.uk\nextendedKeyUsage=serverAuth\nbasicConstraints=critical,CA:FALSE\n' > ext.cnf &&
  openssl x509 -req -in suite.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 1 -extfile ext.cnf -out suite.crt &&
  chmod 644 suite.key)
IMAGE_TAG=release-v5.3.2 docker compose --project-name oidc-conformance \
  -f conformance-suite/docker-compose-prebuilt.yml \
  -f <stargate>/.github/oidc-conformance/compose.override.yml up -d

# 2. Auth app (prerendered login and logout pages), built into <stargate>/.stargate/apps/auth
(cd <stargate>/apps/auth && npm ci && VITE_API_URL= npm run build)

# 3. Stargate with TLS, from a scratch directory
mkdir -p .stargate && cp <stargate>/.github/oidc-conformance/stargate-config.yaml .stargate/config.yaml
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj "/CN=stargate.local" \
  -addext "subjectAltName=DNS:stargate.local" -keyout key.pem -out cert.pem
export TLS_ENABLED=true TLS_CERT="$PWD/cert.pem" TLS_KEY="$PWD/key.pem" PORT=5443 \
  JWT_ISSUER=https://stargate.local:5443 MFA_MODE=off \
  AUTH_APP_DIR=<stargate>/.stargate/apps/auth BACKCHANNEL_LOGOUT_CA_FILE="$SUITE_TLS_DIR/ca.crt"
<stargate>/target/debug/stargate admin bootstrap --email oidc-tester@example.com \
  --password 'Blue-Harbor-7319!' --name "Oidc Tester" --nickname tester
<stargate>/target/debug/stargate

# 4. Runner
python3 -m venv venv && venv/bin/pip install -r conformance-suite/scripts/requirements.txt
export CONFORMANCE_SERVER=https://localhost.emobix.co.uk:8443/ CONFORMANCE_DEV_MODE=1
venv/bin/python conformance-suite/scripts/run-test-plan.py \
  oidcc-config-certification-test-plan <stargate>/.github/oidc-conformance/config-op.json

API_URL=https://127.0.0.1:5443 <stargate>/.github/oidc-conformance/op-setup.sh \
  https://stargate.local:5443 oidc-tester@example.com 'Blue-Harbor-7319!' op.json backchannel-op.json
venv/bin/python conformance-suite/scripts/run-test-plan.py \
  --expected-failures-file <stargate>/.github/oidc-conformance/basic-op-expected-failures.json \
  --expected-skips-file <stargate>/.github/oidc-conformance/basic-op-expected-skips.json \
  "oidcc-basic-certification-test-plan[server_metadata=discovery][client_registration=static_client]" \
  op.json
venv/bin/python conformance-suite/scripts/run-test-plan.py \
  "oidcc-rp-initiated-logout-certification-test-plan[response_type=code][client_registration=static_client]" op.json
venv/bin/python conformance-suite/scripts/run-test-plan.py \
  "oidcc-backchannel-rp-initiated-logout-certification-test-plan[response_type=code][client_registration=static_client]" backchannel-op.json
```

The expected-results files match the config by name (`*op.json`).

The suite UI is at `https://localhost.emobix.co.uk:8443` (self-signed
certificate). Stop it with
`docker compose --project-name oidc-conformance down`.

## Certification

Local results cannot be submitted. To certify Basic OP, run the same plan on
`www.certification.openid.net` against a publicly reachable deployment, upload
screenshots for the four `REVIEW` tests, and submit the plan. The logout
profiles work the same way, with the eight RP-Initiated Logout screenshots;
there the deployment must reach the hosted suite's `backchannel_logout` URL. Warnings do not
block certification, but they are visible in the published results.
