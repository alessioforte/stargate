# Stargate User Management System and API Gateway

## Deployment Profiles

Stargate supports different deployment profiles with different operational limits:

- `edge`: single-node deployments where local `sqlite + memory` state is acceptable. This fits Raspberry Pi, appliance, and other edge installs. Use `replicaCount=1` and keep autoscaling disabled.
- `cluster`: multi-replica deployments that require shared backends such as Redis and Postgres. Do not use local-only state assumptions in this mode.

The Helm chart defaults to `deployment.mode=edge` and validates incompatible combinations at template time.

## Build Profiles

The container image builds with an explicit backend profile:

- `edge`: compiles `sqlite + memory`
- `cluster`: compiles `postgres + redis`

```bash
docker build --build-arg STARGATE_PROFILE=edge -t stargate:edge .
docker build --build-arg STARGATE_PROFILE=cluster -t stargate:cluster .
```

At runtime, Stargate validates `STARGATE_RUNTIME_PROFILE` against the compiled backend set and fails fast if they do not match.

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

## Bootstrap The First Super Admin

Use the CLI to create the initial super-admin account explicitly. Stargate no longer auto-creates a privileged user during normal startup.

A profile must be selected on every build/run (`edge` and `cluster` are
mutually exclusive, and there is no default). The examples below use
`--features edge`; swap in `--features cluster` for cluster deployments.

Pass the password directly:

```bash
cargo run --features edge -- admin bootstrap \
  --email admin@example.com \
  --password 'replace-with-a-strong-password' \
  --name 'Admin User' \
  --nickname admin
```

Or read the password from stdin so it does not appear in shell history:

```bash
printf '%s' 'replace-with-a-strong-password' | cargo run --features edge -- admin bootstrap \
  --email admin@example.com \
  --password-stdin \
  --name 'Admin User' \
  --nickname admin
```

Or let Stargate generate a one-time bootstrap password and print it after successful creation:

```bash
cargo run --features edge -- admin bootstrap \
  --email admin@example.com \
  --generate-password \
  --name 'Admin User' \
  --nickname admin
```

If you are running a built binary instead of `cargo run`, use the same arguments:

```bash
./stargate admin bootstrap --email admin@example.com --password-stdin
```

## CLI Behavior

- `admin bootstrap` fails if a super-admin already exists.
- `admin bootstrap` fails if the target email already exists as a user.
- `--password`, `--password-stdin`, and `--generate-password` are mutually exclusive.
- One of `--password`, `--password-stdin`, or `--generate-password` is required.
- `--generate-password` prints the generated password only after a successful bootstrap.
- `--nickname` is optional and defaults to the email address.
- `--name` is optional and sets the initial given name.

## Runtime Requirements

The CLI initializes the database before executing the command, so it uses the same compiled backend profile and runtime database configuration as the server.

- In `edge` mode, ensure the process can create or access `.stargate/sqlite.db`.
- In `cluster` mode, set the required Postgres environment variables before running the command.
- If `STARGATE_RUNTIME_PROFILE` is set, it must match the compiled profile.
