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

## Managed PostgreSQL and TLS

Provision the target database separately when the application role has no
`CREATEDB` privilege or cannot connect to the `postgres` maintenance database.
Set `POSTGRES_CREATE_DATABASE` to `"false"`; schema migrations still run on the
target database. With the default `"true"`, Stargate creates a missing database
using the same credentials and TLS settings as its main pool. Concurrent creation
is accepted only after a new query confirms that the requested database exists.

`POSTGRES_ENDPOINT` accepts `host`, `host:port`, `[ipv6]`, or `[ipv6]:port`, with
default port 5432. Credentials are separate fields: store `POSTGRES_PASSWORD` in
the runtime Secret exactly as supplied, without URL encoding.

`POSTGRES_SSLMODE` accepts `disable`, `prefer` (default), `require`, `verify-ca`,
or `verify-full`. Use `verify-full` to verify both the certificate chain and the
server hostname. For a private CA, mount its PEM certificate read-only and set
`POSTGRES_SSLROOTCERT` to that path:

```yaml
runtimeConfig:
  data:
    POSTGRES_ENDPOINT: postgres.managed.example:5432
    POSTGRES_DATABASE: stargate
    POSTGRES_USERNAME: stargate
    POSTGRES_CREATE_DATABASE: "false"
    POSTGRES_SSLMODE: verify-full
    POSTGRES_SSLROOTCERT: /etc/stargate/postgres/ca.pem
extraVolumes:
  - name: postgres-ca
    configMap:
      name: postgres-ca
extraVolumeMounts:
  - name: postgres-ca
    mountPath: /etc/stargate/postgres
    readOnly: true
```

Create `postgres-ca` with a `ca.pem` key. Mount the CA at the same path in every
CLI process or Job using these settings. Chart `extraVolumes` and
`extraVolumeMounts` apply to the Deployment and migration Job; the bootstrap and config
import hooks do not inherit these mounts. When a private CA is required, use
separate CLI Jobs with the CA mounted instead of enabling those hooks.

## Multiple public hosts

When an ingress forwards both `identity.example.com` and `api.example.com` to
Stargate, enable the [Traefik integration](#pattern-a-traefik-in-front) below,
which manages `IAM_HOSTS` from `traefik.identityHost`. With another ingress
controller, put `IAM_HOSTS: identity.example.com` in `runtimeConfig.data` and set
`JWT_ISSUER` (or `OAUTH_BASE_URL`) to the public identity URL. The public base URL
host must appear in `IAM_HOSTS`, or startup fails. The comma-separated list accepts
exact hostnames only, without schemes, ports, paths, or wildcards; comparisons
ignore case and the request port.

IAM APIs, docs, and hosted apps are served on the identity host. Requests on the
API host go directly to gateway routing, including paths such as `/docs` and
`/admin/me`. `/livez` and `/readyz` stay available on every host for kubelet probes.
Leaving `IAM_HOSTS` unset or empty keeps IAM routes available on every host.
Ingress host rules must forward each intended hostname to the Service.

## Pattern A: Traefik in front

Traefik terminates public TLS and forwards both identity and API hosts to the
same Stargate Service. Each replica dispatches identity-host requests through
IAM and other hosts through gateway routing:

```mermaid
flowchart LR
    Clients -->|HTTPS| Traefik
    Traefik -->|identity and API hosts| Service[Stargate Service]
    Service --> Replicas[Three Stargate replicas]
    Replicas -->|identity host| IAM[IAM, login, Console]
    Replicas -->|API hosts| Gateway[Gateway routes]
```

Install Traefik v3 and its `traefik.io/v1alpha1` CRDs first. This chart creates
an `IngressRoute` when `traefik.enabled=true`; it rejects enabling the standard
`ingress.enabled` at the same time. Minimal routing overrides are:

```yaml
traefik:
  enabled: true
  identityHost: identity.example.com
  apiHosts: [api.example.com]
  tls:
    secretName: stargate-tls
trustedProxies:
  - 10.42.0.0/16  # Replace with the real Traefik peer network.
runtimeConfig:
  data:
    JWT_ISSUER: https://identity.example.com
    OAUTH_BASE_URL: https://identity.example.com
```

The TLS Secret belongs in Stargate's namespace and must cover every configured
host. Alternatively clear `secretName` and set `traefik.tls.certResolver` to a
resolver configured in Traefik. With neither selected, `tls: {}` uses Traefik's
default certificate configuration. Ordinary routes use HTTP to Stargate; setting
`traefik.grpc.enabled=true` adds a higher-priority `HeaderRegexp` route for each
host using h2c. WebSocket upgrades continue through the ordinary HTTP routes.
These are [Traefik IngressRoute options](https://doc.traefik.io/traefik/v3.3/reference/routing-configuration/kubernetes/crd/http/ingressroute/).

Use [values-traefik.yaml](values-traefik.yaml) for the complete three-replica
cluster example. Before installing, adapt the image, hosts, real pod CIDRs,
backend namespaces, and NetworkPolicy selectors/ports, and provision the runtime,
shared JWT, and TLS Secrets. See [cluster deployment](#production-cluster-deployment)
and [JWT keys](#jwt-signing-keys). Then:

```bash
helm lint ./k8s/stargate -f ./k8s/stargate/values-traefik.yaml
helm template stargate ./k8s/stargate -f ./k8s/stargate/values-traefik.yaml
helm upgrade --install stargate ./k8s/stargate \
  --namespace identity --create-namespace \
  --values ./k8s/stargate/values-traefik.yaml --atomic --timeout 10m
```

The example enables a NetworkPolicy whose ingress peer combines the
`kube-system` namespace selector and the Traefik pod selector in the same peer.
Egress is limited to DNS, PostgreSQL, Redis, SMTP, OTLP, and backend namespaces.
Use a network-policy-enforcing CNI and adapt these rules for managed services,
NodeLocal DNS, external OAuth/JWKS, and additional upstreams. Keep backend ingress
policies restricted to Stargate as well. The gateway ConfigMap is only a seed;
later edits use the database-backed admin API or CLI, as described under
[gateway configuration](#gateway-configuration).

To restrict the identity host's admin API and Console, enable the Traefik IP
allow-list and replace these example ranges with the addresses administrators
use from their browsers (for example, their VPN or corporate network):

```yaml
traefik:
  adminAllowList:
    enabled: true
    sourceRange: [203.0.113.0/24, 2001:db8::/32]
    ipStrategyDepth: 0
```

The allow-list is disabled by default. When enabled, it requires a nonempty list
of valid IP addresses or CIDRs. The Console calls `/admin/*` from the browser;
allowing only server addresses will not allow administrators to use it. The
canonical login paths `/auth` and `/oauth` remain public. The protected route has
priority over the ordinary and gRPC routes. The public identity routes exclude
the protected paths, so a missing or invalid middleware cannot expose them by
falling through to a public route.

The route also applies the allow-list to potentially ambiguous identity-host paths:
those containing repeated slashes, percent escapes or dot segments, and
case-insensitive variants of the protected prefixes. Use canonical paths for
public endpoints. Keep Traefik's path sanitization enabled and use Traefik 3.4.1
or later; see [Traefik's path handling](https://doc.traefik.io/traefik/v3.6/security/request-path/).
If `CONSOLE_APP_BASE_PATH` is changed, protect that additional path separately.
The middleware covers requests through this IngressRoute only: restrict direct
Service access and any other ingress routes, for example with the NetworkPolicy
in `values-traefik.yaml`.

With `ipStrategyDepth: 0`, Traefik checks the connecting peer's address. A
positive depth selects an address from `X-Forwarded-For`, counting from the
right; set it for the actual proxy chain and configure Traefik's own trusted
forwarded-header peers so clients cannot forge that chain. This setting is
separate from Stargate's `trustedProxies`. A depth beyond the available chain
does not select an address. Verify access from both permitted and other networks
after deployment. See [Traefik's IP allow-list](https://doc.traefik.io/traefik/v3.5/reference/routing-configuration/http/middlewares/ipallowlist/).

Preserve the real client IP on **Traefik's** public Service with
`externalTrafficPolicy: Local`, and set Stargate's `trustedProxies` to the actual
Traefik peer CIDRs. Local traffic policy requires traffic to reach nodes with
local Traefik endpoints. On k3s, merge the following into the existing
`HelmChartConfig` named `traefik` in `kube-system`, or create it if absent; do not
edit k3s's generated Traefik manifest. See [k3s networking](https://docs.k3s.io/networking/networking-services#traefik-ingress-controller)
and the [Traefik chart's Service settings](https://github.com/traefik/traefik-helm-chart/blob/master/traefik/values.yaml).

```yaml
apiVersion: helm.cattle.io/v1
kind: HelmChartConfig
metadata:
  name: traefik
  namespace: kube-system
spec:
  valuesContent: |-
    service:
      spec:
        externalTrafficPolicy: Local
    additionalArguments:
      - --entrypoints.websecure.transport.respondingtimeouts.readtimeout=600s
      - --entrypoints.websecure.transport.respondingtimeouts.writetimeout=0s
      - --serverstransport.forwardingtimeouts.responseheadertimeout=35s
```

These timeout values are examples, not deployment defaults. Traefik v3's
`entryPoints.<name>.transport.respondingTimeouts.readTimeout` defaults to 60s and
covers the full incoming request body; increase it for long uploads. Keep
`writeTimeout` suitable for streams. See [entryPoint timeouts](https://doc.traefik.io/traefik/v3.0/routing/entrypoints/#respondingtimeouts).
Align `serversTransport.forwardingTimeouts.responseHeaderTimeout` with the
Stargate request/header budgets in [config v1](../../docs/config-v1.md#runtime-deadlines-and-resource-budgets),
including processing before headers; it does not bound the response body.
Traefik's default is 0s (no header timeout). The example 35s is above Stargate's
default 30s upstream-header budget; adjust both for the actual workload. See
[forwarding timeout semantics](https://doc.traefik.io/traefik/v3.0/routing/services/#forwardingtimeouts).

For Stargate upstream targets pointing to a backend Service ClusterIP, set
`transport.protocols: [http1]`, as the example gateway seed does. A TCP connection
is assigned to one backend pod; multiplexing requests on a persistent HTTP/2
connection concentrates them on that pod. HTTP/1.1 permits distribution across
the client's connection pool. This follows [Kubernetes Service proxy behavior](https://kubernetes.io/docs/reference/networking/virtual-ips/#proxy-mode-iptables).

To provision the optional internal-context signer, generate material explicitly
and create its Secret in the release namespace:

```bash
stargate internal-context generate-key --output-dir ./internal-context --kid gateway-2026-10
kubectl -n identity create secret generic stargate-internal-context \
  --from-file=private.pem=./internal-context/private.pem \
  --from-file=jwks.json=./internal-context/jwks.json
```

```yaml
internalContext:
  enabled: true
  existingSecret: stargate-internal-context
  issuer: https://identity.example.com
  kid: gateway-2026-10
```

The chart mounts only `private.pem` and `jwks.json` read-only at
`internalContext.mountPath` and sets the four signer path/issuer/kid variables.
Keep the private key out of values and ConfigMaps. Restart replicas after
replacing external key material; see [internal-context operations](../../docs/internal-context-operations.md)
for consumer trust and rotation. Add `internal_context.audience` to selected
gateway upstreams when they should receive signed context.

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

## Database migrations

By default, every process initializes the schema on startup. In cluster mode,
enable the optional migration Job to run migrations before each install or
upgrade, then let the server and other release Jobs verify the schema without
running migrations:

```yaml
migrations:
  job:
    enabled: true
    existingSecret: stargate-migration-credentials
```

Create the optional Secret before installing; its `POSTGRES_USERNAME` and
`POSTGRES_PASSWORD` keys must name a role allowed to apply schema changes. If
`existingSecret` is empty, the Job uses the application credentials. For an
already provisioned database, set `POSTGRES_CREATE_DATABASE: "false"` to avoid
requiring database creation privileges. The application role needs read access
to `_sqlx_migrations` for the schema check. The chart sets
`DB_MIGRATE_ON_START=false` explicitly in the Deployment, bootstrap Job, and
import Job when migrations are enabled; this takes precedence over `envFrom`,
including external ConfigMaps and Secrets.

The Job and its dedicated copies of chart-created runtime ConfigMaps and Secrets
are pre-install and pre-upgrade hooks. They use the new release's values. An
external runtime ConfigMap or Secret is referenced directly and must exist before
`helm install`; otherwise the Job and Deployment cannot start and report
`CreateContainerConfigError`. An existing migration Secret and any resources referenced
by `extraEnv`, `extraEnvFrom` or `extraVolumes` must already exist before
installation. The Job uses the
namespace's default ServiceAccount without a mounted token. It has distinct
labels from server pods; if the namespace denies egress by default, allow the
Job access to DNS and PostgreSQL separately. A failed Job fails the install or
upgrade before the rollout and leaves existing pods unchanged. Hook resources
may remain after a failed run and are replaced on the next attempt.

During a rolling upgrade, old replicas continue using the upgraded schema.
Migrations must therefore remain compatible with the previous release: add
new structures first and remove old ones in a later release. The Job is
cluster-only; edge applies SQLite migrations at startup.

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
test use `/readyz`. Readiness checks SQLite in edge mode and PostgreSQL in
cluster mode, with a one-second SQL timeout; Redis does not gate readiness.
The public probes bypass authentication and rate limiting.

When Redis is down, ready replicas remain in service, but all requests except
the probes fail closed at the ingress limiter with Stargate's `503` and
`Retry-After: 10`, including static pages and gateway direct responses. Traffic
can recover as soon as Redis returns. `/admin/health` reports Redis health when
its handler is reached, but its own request is blocked by the IAM ingress
limiter during a Redis outage. See
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
- `DB_MIGRATE_ON_START` when `migrations.job.enabled` (set that option)
- `JWT_KID` and `JWT_ID_TOKEN_KID` (set `jwt.activeKid` and `jwt.idTokenKid`)
- `IAM_HOSTS` when `traefik.enabled` (set `traefik.identityHost`)
- `TRUSTED_PROXIES` when `trustedProxies` is nonempty (set that list)
- `INTERNAL_CONTEXT_PRIVATE_KEY_PATH` and `INTERNAL_CONTEXT_JWKS_PATH` when
  `internalContext.enabled` (set `internalContext.existingSecret` and `mountPath`)
- `INTERNAL_CONTEXT_ISSUER` and `INTERNAL_CONTEXT_KID` when `internalContext.enabled`
  (set `internalContext.issuer` and `kid`)

The conditional keys above are also rejected in `extraEnv` while generated by
the chart. Without their corresponding chart option enabled, they can be
supplied manually, for example `IAM_HOSTS` with a different ingress controller.
Do not duplicate managed keys in external ConfigMaps, Secrets, or `extraEnvFrom`;
the chart cannot inspect externally managed resources.

It also rejects known confidential keys in `runtimeConfig.data`, including
database passwords, authenticated Redis URLs, JWT secrets, OTP/password
peppers, SMTP passwords, and the Google/GitHub OAuth client secrets.

Use:

- `runtimeConfig.data` for non-confidential environment variables.
- `runtimeSecret.existingSecret` for confidential runtime values.
- `extraEnv` and `extraEnvFrom` for additional integrations.
- `gatewayConfig` for gateway routes and ACE policies.
- `jwt` for signing keys.
- `traefik`, `trustedProxies`, and `internalContext` for their managed settings.

## Validation

```bash
helm lint ./k8s/stargate
helm lint ./k8s/stargate -f ./k8s/stargate/values-cluster.yaml
helm lint ./k8s/stargate -f ./k8s/stargate/values-traefik.yaml
helm template edge ./k8s/stargate
helm template cluster ./k8s/stargate -f ./k8s/stargate/values-cluster.yaml
helm template pattern-a ./k8s/stargate -f ./k8s/stargate/values-traefik.yaml
helm lint ./k8s/stargate -f ./k8s/stargate/values-traefik.yaml --set traefik.grpc.enabled=true
helm template pattern-a ./k8s/stargate -f ./k8s/stargate/values-traefik.yaml --set traefik.grpc.enabled=true
helm lint ./k8s/stargate -f ./k8s/stargate/values-traefik.yaml --set traefik.adminAllowList.enabled=true --set 'traefik.adminAllowList.sourceRange[0]=192.0.2.0/24'
helm template pattern-a ./k8s/stargate -f ./k8s/stargate/values-traefik.yaml --set traefik.adminAllowList.enabled=true --set 'traefik.adminAllowList.sourceRange[0]=192.0.2.0/24'
python3 ./k8s/stargate/tests/test_admin_allowlist.py
# Expected failure: an enabled allow-list needs at least one source range.
helm template pattern-a ./k8s/stargate -f ./k8s/stargate/values-traefik.yaml --set traefik.adminAllowList.enabled=true
helm lint ./k8s/stargate -f ./k8s/stargate/values-traefik.yaml --set internalContext.enabled=true --set internalContext.existingSecret=stargate-internal-context --set internalContext.issuer=https://identity.example.com --set internalContext.kid=gateway-2026-10
helm template pattern-a ./k8s/stargate -f ./k8s/stargate/values-traefik.yaml --set internalContext.enabled=true --set internalContext.existingSecret=stargate-internal-context --set internalContext.issuer=https://identity.example.com --set internalContext.kid=gateway-2026-10
helm lint ./k8s/stargate -f ./k8s/stargate/values-cluster.yaml --set gatewayConfig.importOnUpgrade=true
helm template cluster ./k8s/stargate -f ./k8s/stargate/values-cluster.yaml --set gatewayConfig.importOnUpgrade=true
helm lint ./k8s/stargate -f ./k8s/stargate/values-cluster.yaml --set migrations.job.enabled=true
helm template cluster ./k8s/stargate -f ./k8s/stargate/values-cluster.yaml --set migrations.job.enabled=true
python3 ./k8s/stargate/tests/test_migrations_job.py  # Requires Python and Ruby.
# Expected failure: migration Jobs require the cluster profile.
helm template edge ./k8s/stargate --set migrations.job.enabled=true
# Expected failure with a kubectl exec alternative:
helm template edge ./k8s/stargate --set gatewayConfig.importOnUpgrade=true
# Expected failure: choose one ingress integration.
helm template pattern-a ./k8s/stargate -f ./k8s/stargate/values-traefik.yaml --set ingress.enabled=true
# Direct-Service probe for policies allowing the test pod:
helm test stargate --namespace identity
```

The existing Helm connection-test pod contacts the Stargate Service directly;
it is outside the Traefik-only ingress peers in `values-traefik.yaml`. For that
example, check health through the public Traefik routes after deployment:

```bash
curl --fail https://identity.example.com/livez
curl --fail https://identity.example.com/readyz
curl --fail https://api.example.com/readyz
```
