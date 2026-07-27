# Stargate User Management System and API Gateway

## Deployment Profiles

Stargate supports different deployment profiles with different operational limits:

- `edge`: single-node deployments where local `sqlite + memory` state is acceptable. This fits Raspberry Pi, appliance, and other edge installs. Use `replicaCount=1` and keep autoscaling disabled.
- `cluster`: multi-replica deployments that require shared backends such as Redis and Postgres. Do not use local-only state assumptions in this mode.

The Helm chart defaults to `deployment.mode=edge` and validates incompatible combinations at template time.

## Docker

The Dockerfile builds the Rust server, the `admin` and `auth` web apps, and the
transactional mail templates. The final image serves:

- Admin UI: `/stargate`
- Auth UI: `/auth`
- API routes and health endpoint: `/account/*`, `/admin/*`, and `/health`

### Build an image

Choose exactly one backend profile:

```bash
# SQLite + in-memory store
docker build \
  --build-arg STARGATE_PROFILE=edge \
  -t stargate:edge .

# PostgreSQL + Redis
docker build \
  --build-arg STARGATE_PROFILE=cluster \
  -t stargate:cluster .
```

The main build arguments are:

| Argument | Default | Purpose |
| --- | --- | --- |
| `STARGATE_PROFILE` | `edge` | Compiles the `edge` or `cluster` backend |
| `ADMIN_APP_BASE_PATH` | `/stargate` | Build and runtime path for the admin UI |
| `AUTH_APP_BASE_PATH` | `/auth` | Build and runtime path for the auth UI |
| `VITE_API_URL` | same browser origin | Public API URL embedded in both web apps |
| `VITE_AUTH_URL` | `<origin>/auth` | Public auth-app URL embedded in the admin UI |
| `VITE_OAUTH_CLIENT_ID` | `stargate_admin` | OAuth client used by the admin UI and instance bootstrap |
| `VITE_OAUTH_REDIRECT_URI` | `<origin><ADMIN_APP_BASE_PATH>/auth/callback` | Admin OAuth callback used by the UI and instance bootstrap |

For a deployment with explicit public URLs:

```bash
docker build \
  --build-arg STARGATE_PROFILE=cluster \
  --build-arg VITE_API_URL=https://identity.example.com \
  --build-arg VITE_AUTH_URL=https://identity.example.com/auth \
  --build-arg VITE_OAUTH_CLIENT_ID=stargate_admin \
  --build-arg VITE_OAUTH_REDIRECT_URI=https://identity.example.com/stargate/auth/callback \
  -t stargate:cluster .
```

`ADMIN_APP_BASE_PATH` and `AUTH_APP_BASE_PATH` are also written into the image's
runtime environment, keeping the Vite asset paths and Rust routes synchronized.
The `VITE_*` values are compiled into the web apps; rebuild the image to change
them.

### Run the edge image

Create a valid v2 gateway configuration first; see
[docs/config-v2alpha1.md](docs/config-v2alpha1.md). The bind-mounted file must
exist before starting the container. Copy `.env.example` to `.env.docker` and
adjust its public URLs, signing configuration, SMTP settings, and secrets.

```bash
docker volume create stargate-data

docker run --rm \
  --name stargate \
  -p 5050:5050 \
  --env-file .env.docker \
  --mount source=stargate-data,target=/app/.stargate \
  --mount type=bind,source="$(pwd)/config.yaml",target=/app/.stargate/config.yaml,readonly \
  stargate:edge
```

The named volume persists SQLite, the in-memory store snapshot, generated key
material, policies, and logs. On its first use, Docker initializes the empty
volume with the admin UI, auth UI, and mail templates bundled in the image.

If the admin configuration API must update `config.yaml`, remove `readonly` and
make the host file writable by container UID/GID `999`. The final image runs as
the non-root `stargate` user.

Open:

- `http://localhost:5050/stargate` for the admin UI
- `http://localhost:5050/auth` for the auth UI
- `http://localhost:5050/health` for the health endpoint

Bootstrap the first administrator and the Admin UI OAuth client while the
container is running:

```bash
docker exec stargate /bin/server admin bootstrap \
  --email admin@example.com \
  --generate-password \
  --name "Admin User" \
  --nickname admin \
  --oauth-redirect-uri http://localhost:5050/stargate/auth/callback
```

The command runs as a second process, uses the server's database configuration,
and exits without stopping the server.

### Run the cluster image

Configure PostgreSQL and Redis in the environment file, then run the image with
the same gateway configuration mount:

```bash
docker run --rm \
  --name stargate \
  -p 5050:5050 \
  --env-file .env.cluster \
  --mount type=bind,source="$(pwd)/config.yaml",target=/app/.stargate/config.yaml,readonly \
  stargate:cluster
```

At minimum, the cluster environment must provide `POSTGRES_ENDPOINT`,
`POSTGRES_DATABASE`, `POSTGRES_USERNAME`, `POSTGRES_PASSWORD`, and `REDIS_URL`.
Use service/container DNS names rather than `localhost` when the databases run
in other containers. Also provide stable JWT signing material: configure an
HMAC `JWT_SECRET`, or mount the asymmetric key directory at
`/app/.stargate/jwks`. Otherwise, tokens issued before a container replacement
will no longer verify.

### Persistent-volume upgrades

Do not bind-mount an empty host directory over `/app/.stargate`: it hides the
web apps and mail templates included in the image. An existing named volume also
retains the artifacts copied from the image when that volume was first created.
When upgrading the image, migrate persistent state to a newly initialized
volume or explicitly refresh these directories:

```text
/app/.stargate/apps/admin
/app/.stargate/apps/auth
/app/.stargate/transactional
```

At runtime, Stargate validates `STARGATE_RUNTIME_PROFILE` against the compiled
backend set and fails fast if they do not match.

## Gateway Config

`config.yaml` uses the v2 schema:

```yaml
schema: stargate/v2alpha1
```

The v2 format models `upstreams`, `services`, `middlewares`, `policies`, and `routers` separately. It supports header/query/cookie/source-IP routing, regex and template paths, explicit route priority, fallback direct responses, path rewriting, weighted services, mirror traffic, and failover on selected response status codes.

See [docs/config-v2alpha1.md](docs/config-v2alpha1.md) for examples and runtime notes.

## CLI

Stargate includes a built-in CLI for administrative tasks. The server checks CLI arguments first, and when a command is provided it runs the CLI flow instead of starting the HTTP server.

Current command tree:

```text
stargate admin bootstrap
```

## Bootstrap The Instance

Use the CLI to create the initial super-admin account and the public OAuth
client required by the Admin UI. Stargate does not auto-create privileged
resources during normal startup.

A profile must be selected on every build/run (`edge` and `cluster` are
mutually exclusive, and there is no default). The examples below use
`--features edge`; swap in `--features cluster` for cluster deployments.

Pass the password directly:

```bash
cargo run --features edge -- admin bootstrap \
  --email admin@example.com \
  --password 'replace-with-a-strong-password' \
  --name 'Admin User' \
  --nickname admin \
  --oauth-redirect-uri https://identity.example.com/stargate/auth/callback
```

Or read the password from stdin so it does not appear in shell history:

```bash
printf '%s' 'replace-with-a-strong-password' | cargo run --features edge -- admin bootstrap \
  --email admin@example.com \
  --password-stdin \
  --name 'Admin User' \
  --nickname admin \
  --oauth-redirect-uri https://identity.example.com/stargate/auth/callback
```

Or let Stargate generate a one-time bootstrap password and print it after successful creation:

```bash
cargo run --features edge -- admin bootstrap \
  --email admin@example.com \
  --generate-password \
  --name 'Admin User' \
  --nickname admin \
  --oauth-redirect-uri https://identity.example.com/stargate/auth/callback
```

If you are running a built binary instead of `cargo run`, use the same arguments:

```bash
./stargate admin bootstrap \
  --email admin@example.com \
  --password-stdin \
  --oauth-redirect-uri https://identity.example.com/stargate/auth/callback
```

## CLI Behavior

- `admin bootstrap` creates the first super-admin and the Admin UI OAuth client
  in one database transaction.
- The OAuth client is a public Authorization Code + PKCE client with refresh
  tokens and the `openid`, `email`, `profile`, and `offline_access` scopes.
- `--oauth-client-id` defaults to `ADMIN_OAUTH_CLIENT_ID`, then
  `stargate_admin`.
- `--oauth-redirect-uri` defaults to `ADMIN_OAUTH_REDIRECT_URI`. When it is
  unset, Stargate derives the callback from `ADMIN_PUBLIC_URL`,
  `OAUTH_BASE_URL`, or `JWT_ISSUER`, plus `ADMIN_APP_BASE_PATH`.
- Production redirect URIs must use HTTPS. HTTP is accepted only for loopback
  hosts such as `localhost`.
- Re-running the command leaves matching resources unchanged.
- When a super-admin already exists but the Admin OAuth client is missing, run
  the command with only the OAuth options to repair it:

  ```bash
  ./stargate admin bootstrap \
    --oauth-redirect-uri https://identity.example.com/stargate/auth/callback
  ```

- If the configured client ID already exists with incompatible settings, the
  command fails without changing the administrator.
- `admin bootstrap` fails if the target email already exists as a user.
- `--password`, `--password-stdin`, and `--generate-password` are mutually exclusive.
- An email and one password option are required only when no super-admin exists.
- `--generate-password` prints the generated password only after a successful bootstrap.
- `--nickname` is optional and defaults to the email address.
- `--name` is optional and sets the initial given name.

## Runtime Requirements

The CLI initializes the database before executing the command, so it uses the same compiled backend profile and runtime database configuration as the server.

- In `edge` mode, ensure the process can create or access `.stargate/sqlite.db`.
- In `cluster` mode, set the required Postgres environment variables before running the command.
- If `STARGATE_RUNTIME_PROFILE` is set, it must match the compiled profile.
