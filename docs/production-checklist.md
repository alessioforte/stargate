# Production checklist (Kubernetes)

Use this list before exposing a Stargate cluster deployment to real users. It
assumes Traefik in front of three cluster replicas, CloudNativePG, and Redis,
installed with `k8s/stargate/values-traefik.yaml` plus
`k8s/stargate/values-production.yaml`:

```bash
helm upgrade --install stargate oci://ghcr.io/<owner>/charts/stargate \
  --version <version> -f values-traefik.yaml -f values-production.yaml
```

`values-production.yaml` is an overlay: maps merge with the Traefik example,
lists such as `networkPolicy` rules and `extraVolumes` replace it. Replace every
example hostname, CIDR, Secret name and image tag.

## Before installing

- [ ] **Image**: a released tag (or digest) built by `release.yml`, not a local build.
- [ ] **Hostnames and TLS**: DNS for the identity and API hosts; a certificate covering
      both, in `traefik.tls.secretName` or through `traefik.tls.certResolver`.
- [ ] **Issuer**: `JWT_ISSUER`, `OAUTH_BASE_URL` and `CONSOLE_PUBLIC_URL` use the public
      `https://` identity URL. It decides `Secure` cookies and HSTS behind Traefik.
- [ ] **Trusted proxies**: `trustedProxies` lists only Traefik's pod network.
- [ ] **Runtime Secret** (`runtimeSecret.existingSecret`) with `POSTGRES_PASSWORD`,
      `REDIS_URL` (`rediss://...`), `REDIS_PASSWORD`, `PASSWORD_PEPPER`,
      `EMAIL_OTP_PEPPER`, `SMTP_USERNAME`, `SMTP_PASSWORD`, and the client secrets of any
      enabled social provider. Store the peppers in your secret manager as well:
      changing `PASSWORD_PEPPER` invalidates every stored password hash.
- [ ] **Migration Secret** (`migrations.job.existingSecret`) with a schema-owner role
      (`POSTGRES_USERNAME`, `POSTGRES_PASSWORD`); the runtime role has DML only.
- [ ] **CA bundles**: the CNPG CA Secret (`<cluster>-ca`, key `ca.crt`) and the Redis CA,
      mounted by `extraVolumes`. `POSTGRES_SSLMODE=verify-full` and `REDIS_CA_FILE`
      refuse unverified connections.
- [ ] **JWT keys**: one Secret per key from `stargate jwt generate-key`, listed in
      `jwt.keys`, with `jwt.activeKid` set. Keep an offline copy.
- [ ] **Internal context** (if backends verify `stargate-context`): signing key Secret from
      `stargate internal-context generate-key`; backends fetch the JWKS through the public
      identity host, never the Stargate Service.
- [ ] **Admin access**: `traefik.adminAllowList.sourceRange` contains only operator
      networks (VPN or office egress).
- [ ] **Email**: an authenticated SMTP relay with SPF and DKIM for the sender domain.
- [ ] **Network policy**: egress rules match your PostgreSQL, Redis, SMTP, collector,
      backends and any external OAuth/JWKS endpoints.

## Settings the overlay enforces

| Setting | Value | Why |
| --- | --- | --- |
| `POSTGRES_SSLMODE` | `verify-full` | The default `prefer` does not verify the server. |
| `POSTGRES_CREATE_DATABASE` | `false` | CNPG creates the database; no maintenance-DB access. |
| `REDIS_CA_FILE` + `rediss://` | private CA | Redis carries sessions and tokens. |
| `MFA_REQUIRED_FOR_SUPER_ADMIN` | `true` | Super-admins sign in with a second factor. |
| `MFA_ADMIN_STEP_UP_REQUIRED` | `true` | Admin operations need a recent step-up. |
| `SIGNUP_MODE` | `invite` | No self-registration unless you choose `open`. |
| `migrations.job.enabled` | `true` | Schema changes run once, before the rollout. |
| `topologySpreadConstraints` | one replica per node | A node failure leaves two replicas. |
| `podDisruptionBudget.maxUnavailable` | `1` | Drains and upgrades keep two replicas serving. |
| `traefik.adminAllowList.enabled` | `true` | Admin API and Console only from operator networks. |
| `LOG_FORMAT` | `json` | One JSON object per line for the log pipeline. |

Traffic limits fail open when Redis is unavailable (`on_store_error: allow`, the
default). Keep a rate limit on Traefik as a safety net during a Redis outage, or set
`on_store_error: deny` where an unlimited window is not acceptable.

## After installing

- [ ] `/readyz` is `200` on every replica; `/admin/health` (admin key) shows healthy SQL
      and Redis, `limits.storeDegraded: false`, and the same `config.gatewayRevision` on
      every replica.
- [ ] Discovery and JWKS answer on the identity host with the public issuer.
- [ ] The first super-admin is bootstrapped, enrols MFA, and signs in to the Console.
- [ ] From outside the allow-list, `/admin/me` and `/stargate` return Traefik `403`.
- [ ] A password reset email arrives with a link on the public identity host.
- [ ] Traces and metrics reach the collector; logs are JSON with `request_id`.
- [ ] The audit outbox drains: `GET /admin/outbox-events` shows published events, and the
      consumer of `AUDIT_REDIS_STREAM` is running.

## Operations

- [ ] PostgreSQL backups to object storage, with a restore tested end to end.
- [ ] A decision on Redis persistence and failover. Losing Redis signs every user out and
      invalidates refresh tokens.
- [ ] Alerts on 5xx rate, `stargate.limits.store_errors`, failed configuration
      activations, audit relay errors and backlog, open circuit breakers, SQL pool
      saturation and certificate expiry.
- [ ] External backups of gateway configuration and ACE rules with
      `stargate config export`. Use `stargate config versions` to find a prior
      version and `config export --version N` to retrieve it. Version history
      stays in the database; exports provide backups outside that database.
- [ ] Documented key rotation for JWT and internal-context keys, rehearsed once.
