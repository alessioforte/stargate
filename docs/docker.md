# Run Stargate with Docker

For a local evaluation, use the [quickstart](../README.md#quickstart): the
repository's `docker-compose.yml` runs the published image with PostgreSQL,
Redis, Mailpit, and a demo upstream. This guide covers building your own image
and running it without Compose.

The Dockerfile builds the Rust server, the `console` and `auth` web apps, and the
transactional mail templates. The final image serves:

- Console: `/stargate`
- Auth UI: `/auth`
- API routes: `/account/*`, `/admin/*`, `/oauth/*`, `/.well-known/*`
- Probes: `/livez` (liveness), `/readyz` (readiness), `/health` (readiness compatibility)

Published release images (`ghcr.io/alessioforte/stargate:<version>`) are built
with the `cluster` profile for `linux/amd64` and `linux/arm64`. There is no
`latest` tag; see [Releasing Stargate](RELEASING.md).

## Build an image

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

| Argument                  | Default                                         | Purpose                                                      |
| ------------------------- | ----------------------------------------------- | ------------------------------------------------------------ |
| `STARGATE_PROFILE`        | `edge`                                          | Compiles the `edge` or `cluster` backend                     |
| `CONSOLE_APP_BASE_PATH`   | `/stargate`                                     | Build and runtime path for the Console                       |
| `AUTH_APP_BASE_PATH`      | `/auth`                                         | Build and runtime path for the auth UI                       |
| `VITE_API_URL`            | same browser origin                             | Public API URL embedded in both web apps                     |
| `VITE_AUTH_URL`           | `<origin>/auth`                                 | Public auth-app URL embedded in the Console                  |
| `VITE_OAUTH_CLIENT_ID`    | `stargate_console`                              | OAuth client used by the Console and instance bootstrap      |
| `VITE_OAUTH_REDIRECT_URI` | `<origin><CONSOLE_APP_BASE_PATH>/auth/callback` | Console OAuth callback used by the UI and instance bootstrap |

For a deployment with explicit public URLs:

```bash
docker build \
  --build-arg STARGATE_PROFILE=cluster \
  --build-arg VITE_API_URL=https://identity.example.com \
  --build-arg VITE_AUTH_URL=https://identity.example.com/auth \
  --build-arg VITE_OAUTH_CLIENT_ID=stargate_console \
  --build-arg VITE_OAUTH_REDIRECT_URI=https://identity.example.com/stargate/auth/callback \
  -t stargate:cluster .
```

`CONSOLE_APP_BASE_PATH` and `AUTH_APP_BASE_PATH` are also written into the image's
runtime environment, keeping the Vite asset paths and Rust routes synchronized.
The `VITE_*` values are compiled into the web apps; rebuild the image to change
them.

## Run the edge image

Create a valid v1 gateway configuration first; see
[Stargate Config v1](config-v1.md) or start from
[`examples/quickstart/config.yaml`](../examples/quickstart/config.yaml). The
bind-mounted file must exist before starting the container. Copy `.env.example`
to `.env.docker` and adjust its public URLs, signing configuration, SMTP
settings, and secrets.

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
volume with the Console, auth UI, and mail templates bundled in the image.

If the admin configuration API must update `config.yaml`, remove `readonly` and
make the host file writable by the container's non-root `stargate` user
(UID/GID `10001`).

Open:

- `http://localhost:5050/stargate` for the Console
- `http://localhost:5050/auth` for the auth UI
- `http://localhost:5050/livez` for process liveness
- `http://localhost:5050/readyz` for traffic readiness

Probe behavior, dependency checks, and shutdown timing are documented in
[Health and readiness probes](health-probes.md).

Bootstrap the first administrator and the Console OAuth client while the
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
and exits without stopping the server. See [CLI](cli.md) for all options.

## Run the cluster image

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
will no longer verify. Every node must see the same key directory; key rotation
(`stargate jwt generate-key`) is described in the
[provider guide](oauth-oidc-provider-guide.md#signing-keys-and-rotation).

## Persistent-volume upgrades

Do not bind-mount an empty host directory over `/app/.stargate`: it hides the
web apps and mail templates included in the image. An existing named volume also
retains the artifacts copied from the image when that volume was first created.
When upgrading the image, migrate persistent state to a newly initialized
volume or explicitly refresh these directories:

```text
/app/.stargate/apps/console
/app/.stargate/apps/auth
/app/.stargate/transactional
```

At runtime, Stargate validates `STARGATE_RUNTIME_PROFILE` against the compiled
backend set and fails fast if they do not match.
