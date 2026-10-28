# Stargate Helm chart

This chart deploys Stargate in one of two explicit profiles:

- `edge`: one replica, SQLite and in-memory state, with a dynamically
  provisioned PVC.
- `cluster`: horizontally scalable PostgreSQL and Redis state, without local
  persistence.

Chart `0.2.0` targets Kubernetes 1.25 or newer.

## Image requirements

The image profile is selected at build time and must match
`deployment.mode`:

```bash
docker build --build-arg STARGATE_PROFILE=edge -t stargate:edge .
docker build --build-arg STARGATE_PROFILE=cluster -t stargate:cluster .
```

The repository Dockerfile uses UID/GID `10001`. The chart enforces that
identity, `runAsNonRoot`, a read-only root filesystem, seccomp
`RuntimeDefault`, no privilege escalation, and no Linux capabilities.

## Edge quick start

The default values create one PVC from the cluster's default StorageClass:

```bash
helm upgrade --install stargate ./k8s/stargate \
  --namespace identity \
  --create-namespace \
  --set image.repository=registry.example.com/stargate \
  --set image.tag=edge
```

The seed init container copies the image-bundled Console, Auth UI, and mail
templates into the claim before Stargate starts. The Deployment uses
`Recreate`, preventing two edge processes from sharing SQLite during an
upgrade.

Use `persistence.existingClaim` to reuse a claim. Do not mount an empty claim
over `/app/.stargate` without `persistence.seed.enabled=true`, or the bundled
applications will be hidden.

## Production cluster deployment

Start from [values-cluster.yaml](values-cluster.yaml):

```bash
helm lint ./k8s/stargate -f ./k8s/stargate/values-cluster.yaml
helm upgrade --install stargate ./k8s/stargate \
  --namespace identity \
  --create-namespace \
  --values ./k8s/stargate/values-cluster.yaml \
  --atomic \
  --timeout 10m
```

Before installing, create the runtime Secret and the first
[JWT signing key](#jwt-signing-keys) Secret:

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: stargate-runtime
  namespace: identity
type: Opaque
stringData:
  POSTGRES_PASSWORD: replace-me
  REDIS_URL: rediss://user:password@redis.example:6379
  EMAIL_OTP_PEPPER: replace-with-a-long-random-value
```

`POSTGRES_ENDPOINT`, `POSTGRES_DATABASE`, and `POSTGRES_USERNAME` may be placed
in `runtimeConfig.data`, as the example does, or in the Secret. An existing
runtime Secret must provide every cluster setting not present in the runtime
ConfigMap.

Prefer an external secret controller or a separately managed Secret in
production. `runtimeSecret.stringData` is intended for development because its
values are stored in Helm release history.

For existing runtime Secrets and runtime ConfigMaps, set the corresponding
`rolloutToken` to a non-secret version or revision that changes with the object.
Chart-owned runtime settings receive automatic checksums. Gateway seed files do
not use a rollout token or Deployment checksum; their import is described below. Terraform can use a Secret's
`metadata.resource_version`; external-secret deployments can use their
controller's restart integration.

## Multiple public hosts

When an ingress forwards both `identity.example.com` and `api.example.com` to
Stargate, put `IAM_HOSTS: identity.example.com` in `runtimeConfig.data` and set
`JWT_ISSUER` (or `OAUTH_BASE_URL`) to the public identity URL. The public base URL
host must appear in `IAM_HOSTS`, or startup fails. The comma-separated list accepts
exact hostnames only, without schemes, ports, paths, or wildcards; comparisons
ignore case and the request port.

IAM APIs, docs, and hosted apps are served on the identity host. Requests on the
API host go directly to gateway routing, including paths such as `/docs` and
`/admin/me`. `/livez` and `/readyz` stay available on every host for kubelet probes.
Leaving `IAM_HOSTS` unset or empty keeps IAM routes available on every host.
Ingress host rules must forward each intended hostname to the Service.

## JWT signing keys

Stargate signs access, refresh, and ID tokens with the keys in
`.stargate/jwks/keys/<kid>/`; relying parties verify them through
`/.well-known/jwks.json`, and OpenID Connect requires RS256 ID tokens.

- **Edge** keeps the keyring on the PVC. The first start generates an RS256
  key (`stargate-current`).
- **Cluster** replicas share no filesystem, so each key is a Secret listed in
  `jwt.keys` and mounted read-only at `.stargate/jwks/keys/<kid>/`. The chart
  sets `JWT_KID` from `jwt.activeKid` and `JWT_ID_TOKEN_KID` from
  `jwt.idTokenKid`.

Create a key with the image's CLI (it needs no database) and store it as a
Secret holding `private.pem`, `public.pem`, and `alg`:

```bash
mkdir -p jwks
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD/jwks:/jwks" \
  ghcr.io/alessioforte/stargate:0.1.0 \
  /bin/server jwt generate-key --kid stargate-2026-10 --jwks-dir /jwks

kubectl -n identity create secret generic stargate-jwt-2026-10 \
  --from-file=jwks/keys/stargate-2026-10/
```

```yaml
jwt:
  activeKid: stargate-2026-10
  keys:
    - kid: stargate-2026-10
      secretName: stargate-jwt-2026-10
```

The `jwks/` directory holds private keys: delete it once the Secrets exist.

Rotate with one `helm upgrade` per step; every replica loads the same keys:

1. Create `stargate-2026-11` and its Secret the same way and append it to
   `jwt.keys`. Stargate publishes it and keeps signing with the current key.
2. Once relying parties have refreshed their JWKS caches (at least
   `JWKS_CACHE_MAX_AGE_SECS`, default 300 seconds), set
   `jwt.activeKid: stargate-2026-11`. Tokens signed by the old key stay valid.
3. After the longest token lifetime (`JWT_REFRESH_EXP`, default one day),
   remove the old entry from `jwt.keys` and delete its Secret.

Each Secret holds one key and is never edited, so every key change alters the
pod template and rolls out. A retired key's Secret may keep only `public.pem`.

With an ES256 active key, also create an RS256 key with `--id-token`, list it
in `jwt.keys`, and set `jwt.idTokenKid`.

HMAC remains available: set `runtimeConfig.data.JWT_ALGORITHM: HS256`, put
`JWT_SECRET` in the runtime Secret, and leave `jwt.activeKid` empty. Relying
parties cannot verify HMAC-signed ID tokens, so also set `jwt.idTokenKid` to an
RS256 key in `jwt.keys`; the install notes warn when it is missing.

On edge, rotate in the pod and restart to publish:

```bash
kubectl -n identity exec deploy/stargate -- /bin/server jwt generate-key --kid stargate-2026-11
kubectl -n identity rollout restart deploy/stargate
```

Then set `jwt.activeKid: stargate-2026-11`, upgrade, and after
`JWT_REFRESH_EXP` remove `.stargate/jwks/keys/stargate-current` in the pod and
restart. `jwt.keys` is cluster-only: a Secret mounted inside the PVC would
leave an empty key directory behind once removed.

## Gateway configuration

Gateway configuration and ACE policies are versioned documents in the database,
managed through the Console, admin API, or `stargate config import`. The ConfigMap
provides read-only seed files at `/etc/stargate`: `config.yaml` and `policies` are
read only when their corresponding database documents do not exist. The gateway
seed is required for an empty database; a missing policy seed means empty rules.
The server never writes these mounted files.

`gatewayConfig.config` must declare `schema: stargate/v1`. The chart rejects
missing/unsupported identifiers, and the server performs full preparation before
activation. For an existing ConfigMap, its keys must match `fileName` and
`policiesFileName` (`policies`). Bind the mandatory ingress limit and retain the
required `default` limit, for example:

```yaml
gatewayConfig:
  config: |
    schema: stargate/v1
    ingress:
      limit: ingress
      timeout: 250ms
    limits:
      default:
        strategy: gcra
        params:
          max_burst: 100
          replenish_1_per: 1s
      ingress:
        strategy: gcra
        params:
          max_burst: 100
          replenish_1_per: 100ms
    http:
      services:
        not-found:
          kind: direct_response
          status: 404
      routers:
        fallback:
          priority: -1000
          match:
            path:
              prefix: /
          service: not-found
  policies: ""
```

Gateway and IAM requests share the per-IP ingress allowance before authentication
and routing; authenticated admin principals also charge their own default bucket.
Probes remain exempt. Set `TRUSTED_PROXIES` to the actual ingress/load-balancer peer
CIDRs so clients behind a trusted proxy retain independent IP allowances.

Changing this ConfigMap does not restart pods or replace database documents.
By default, Console/API edits remain authoritative after Helm upgrades and restarts.
Cluster replicas receive transactional PostgreSQL notifications; both cluster and
edge also check at `CONFIG_SYNC_INTERVAL_SECS` (default 30 seconds).

For cluster GitOps deployments, enable:

```yaml
gatewayConfig:
  importOnUpgrade: true
```

The `post-install,post-upgrade` hook imports both mounted documents through the CLI
using the release image and runtime database settings. It runs after the optional
bootstrap hook (weight 20), retains its last Job for logs, and replaces it on the
next upgrade. Enabling this mode accepts that every Helm upgrade overwrites Console
edits with the mounted files. Unchanged content creates no new version or audit event.
Full TLS/signer preparation happens on each replica, so referenced material must be
mounted there. Capacity changes still require a pod restart.

`importOnUpgrade` is rejected in edge mode: a second pod must not mount the SQLite
ReadWriteOnce PVC. Import inside the existing pod instead, once the updated files
are visible in its ConfigMap mount:

```bash
kubectl exec -n identity deployment/stargate -- /bin/server config import --kind gateway --file /etc/stargate/config.yaml
kubectl exec -n identity deployment/stargate -- /bin/server config import --kind policies --file /etc/stargate/policies
kubectl exec -n identity deployment/stargate -- /bin/server config export --kind gateway > gateway-backup.yaml
```

Edge applies external imports on the next periodic check without restarting. Use
`/admin/health` to inspect active gateway/policy revisions. See [CLI operations](../../docs/cli.md)
and the [configuration reference](../../docs/config-v1.md#database-seed-files-and-admin-updates).

## Bootstrap the first administrator

Stargate does not create privileged resources during server startup. The
optional hook Job runs the idempotent `admin bootstrap` command after installs
and upgrades.

Create a dedicated Secret:

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: stargate-bootstrap
  namespace: identity
type: Opaque
stringData:
  password: replace-with-the-initial-password
```

Enable the hook:

```yaml
bootstrap:
  enabled: true
  email: admin@example.com
  oauthClientId: stargate_console
  oauthRedirectUri: https://identity.example.com/stargate/auth/callback
  existingSecret: stargate-bootstrap
```

The client ID and redirect URI must match the Console image's
`VITE_OAUTH_CLIENT_ID` and `VITE_OAUTH_REDIRECT_URI` build configuration. The
Job uses different labels from the server pods and can never be selected by
the Service.

## Network and availability controls

- The Service defaults to `ClusterIP`.
- Startup, liveness, and readiness probes are enabled.
- Cluster deployments default to rolling updates; enable
  `podDisruptionBudget`.
- `topologySpreadConstraints`, affinity, tolerations, sidecars, init
  containers, extra volumes, and extra environment sources are configurable.
- `networkPolicy.enabled=true` applies exactly the supplied ingress/egress
  rules. Empty rules deny that traffic direction. Allow DNS, PostgreSQL,
  Redis, SMTP, upstream APIs, telemetry, and ingress-controller traffic as
  required by the architecture.

Startup and liveness probes use `/livez`; readiness and the Helm connection
test use `/readyz`. Readiness checks SQLite in edge mode and PostgreSQL plus
Redis in cluster mode, with concurrent one-second dependency timeouts.
The public probes bypass authentication and rate limiting.

`/admin/health` provides authenticated dependency diagnostics. See
[Health and readiness probes](../../docs/health-probes.md) for response contracts
and shutdown behavior. `SERVER_DRAIN_DELAY_SECS` defaults to 5 seconds;
allow room in `terminationGracePeriodSeconds` for that delay, connection
draining, and final persistence hooks. The chart defaults to 40 seconds.

## Features

Sign-up, passwordless login, social sign-in, and the Console are switched with
environment variables ([Features](../../docs/features.md)). Put the switches in
`runtimeConfig.data` and their secrets in the runtime Secret:

```yaml
runtimeConfig:
  data:
    SIGNUP_MODE: open                 # open, invite (default), or closed
    SIGNUP_ALLOWED_DOMAINS: example.com
    PASSWORDLESS_LOGIN_ENABLED: "true"
    CONSOLE_ENABLED: "false"          # the admin API stays available
    GITHUB_OAUTH_CLIENT_ID: Iv1.0123456789abcdef
runtimeSecret:
  existingSecret: stargate-runtime    # EMAIL_OTP_PEPPER, GITHUB_OAUTH_CLIENT_SECRET
```

An invalid or contradictory value stops the pod at startup with a message
naming the variable: for example passwordless login without
`EMAIL_OTP_PEPPER`, or a client id without its secret. The container exits
and restarts (`CrashLoopBackOff`); `kubectl logs` shows the reason.

## Configuration ownership

The chart manages these environment variables and rejects duplicates in
`runtimeConfig.data`:

- `STARGATE_RUNTIME_PROFILE`
- `PORT`
- `CONFIG_PATH`
- `CONFIG_FILENAME`
- `JWT_KID` and `JWT_ID_TOKEN_KID` (set `jwt.activeKid` and `jwt.idTokenKid`)

It also rejects known confidential keys in `runtimeConfig.data`, including
database passwords, authenticated Redis URLs, JWT secrets, OTP/password
peppers, SMTP passwords, and the Google/GitHub OAuth client secrets.

Use:

- `runtimeConfig.data` for non-confidential environment variables.
- `runtimeSecret.existingSecret` for confidential runtime values.
- `extraEnv` and `extraEnvFrom` for additional integrations.
- `gatewayConfig` for gateway routes and ACE policies.
- `jwt` for signing keys.

## Validation

```bash
helm lint ./k8s/stargate
helm lint ./k8s/stargate -f ./k8s/stargate/values-cluster.yaml
helm template edge ./k8s/stargate
helm template cluster ./k8s/stargate -f ./k8s/stargate/values-cluster.yaml
helm lint ./k8s/stargate -f ./k8s/stargate/values-cluster.yaml --set gatewayConfig.importOnUpgrade=true
helm template cluster ./k8s/stargate -f ./k8s/stargate/values-cluster.yaml --set gatewayConfig.importOnUpgrade=true
# Expected failure with a kubectl exec alternative:
helm template edge ./k8s/stargate --set gatewayConfig.importOnUpgrade=true
helm test stargate --namespace identity
```
