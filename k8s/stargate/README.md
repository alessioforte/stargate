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

The seed init container copies the image-bundled Admin UI, Auth UI, and mail
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

Before installing, create the runtime Secret:

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
  JWT_SECRET: replace-with-a-long-random-value
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

## Gateway configuration

`gatewayConfig.config` must be a valid `stargate/v2alpha1` document. The chart
also creates the initial `policies` file and mounts both files read-only at
`/etc/stargate`.

```yaml
gatewayConfig:
  config: |
    schema: stargate/v2alpha1
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
  oauthClientId: stargate_admin
  oauthRedirectUri: https://identity.example.com/stargate/auth/callback
  existingSecret: stargate-bootstrap
```

The client ID and redirect URI must match the Admin UI image's
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

`/health` follows readiness semantics while retaining its legacy JSON fields.
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

It also rejects known confidential keys in `runtimeConfig.data`, including
database passwords, authenticated Redis URLs, JWT secrets, OTP/password
peppers, and SMTP passwords.

Use:

- `runtimeConfig.data` for non-confidential environment variables.
- `runtimeSecret.existingSecret` for confidential runtime values.
- `extraEnv` and `extraEnvFrom` for additional integrations.
- `gatewayConfig` for gateway routes and ACE policies.

## Upgrade from chart 0.1.x

Chart `0.2.0` intentionally removes unsafe values and resources:

| 0.1.x | 0.2.0 |
| --- | --- |
| `configmap.*` | `runtimeConfig.*` and `runtimeSecret.*` |
| `persistentVolume*` | `persistence.*` |
| `volumes` / `volumeMounts` | `persistence` or `extraVolumes` / `extraVolumeMounts` |
| Static `stargate-pv` hostPath | Dynamic PVC or `persistence.existingClaim` |
| `securityContext` | `containerSecurityContext` |
| `livenessProbe` / `readinessProbe` | `probes.*` |

The image now runs as numeric UID/GID `10001`. Confirm that reused volumes can
be written by that identity or by `fsGroup: 10001`.

Because the old Deployment selector does not change, normal Helm upgrades can
retain the Deployment. For an old edge release named `stargate`, set
`persistence.existingClaim=stargate-pvc` to reuse its claim. The removed static
PV was cluster-scoped and Helm-managed; back up or migrate its data before
upgrading. Review the rendered diff before applying it.

## Validation

```bash
helm lint ./k8s/stargate
helm lint ./k8s/stargate -f ./k8s/stargate/values-cluster.yaml
helm template edge ./k8s/stargate
helm template cluster ./k8s/stargate -f ./k8s/stargate/values-cluster.yaml
helm test stargate --namespace identity
```
