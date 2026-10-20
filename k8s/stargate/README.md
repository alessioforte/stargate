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

For existing Secrets and ConfigMaps, set the corresponding `rolloutToken` to a
non-secret version or revision that changes with the object. Chart-owned
configuration receives automatic checksums. Terraform can use a Secret's
`metadata.resource_version`; external-secret deployments can use their
controller's restart integration.

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

`gatewayConfig.config` must be a valid `stargate/v1` document. The chart
also creates the initial `policies` file and mounts both files read-only at
`/etc/stargate`. Chart rendering rejects missing or unsupported schema
identifiers; Stargate validates the full configuration at startup.

Bind the mandatory `ingress.limit` to a named rate limit. The chart defaults
allow a burst of 100 per resolved client IP and replenish 10 requests/s, with
a `250ms` store deadline. Gateway and IAM requests share this allowance and
consume it before authentication/routing; authenticated `/admin/*` principals
also charge their own `default` bucket. Probes remain exempt. Behind an ingress
controller or load balancer, set `TRUSTED_PROXIES` to the actual proxy peer
CIDRs: otherwise every client resolves to the proxy's address and shares one
allowance.

```yaml
gatewayConfig:
  config: |
    schema: stargate/v1
    ingress:
      limit: ingress
      timeout: 250ms
    limits:
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

For GitOps deployments, update these values and let Helm roll the pods using
the generated checksum annotation. The Admin API must not edit the mounted
files. If runtime editing is required, set `gatewayConfig.create=false`, point
`gatewayConfig.existingConfigMap` at an operator-managed ConfigMap, and provide
a separate writable persistence design.

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
peppers, and SMTP passwords.

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
helm test stargate --namespace identity
```
