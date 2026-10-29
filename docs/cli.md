# Stargate CLI

Stargate includes a built-in CLI for administrative tasks. The server checks CLI
arguments first, and when a command is provided it runs the CLI flow instead of
starting the HTTP server.

Current command tree:

```text
stargate admin bootstrap
stargate config export --kind gateway|policies [--output <file>]
stargate config import --kind gateway|policies --file <file>
stargate db migrate
stargate internal-context generate-key
stargate jwt generate-key
```

`internal-context generate-key` is documented in
[Internal context operations](internal-context-operations.md).

`jwt generate-key --kid <kid> [--algorithm ES256] [--id-token] [--jwks-dir .stargate/jwks]`
creates the next JWT signing key in `keys/<kid>/` (refusing to overwrite) and
prints the rotation steps. With `--id-token` the key is RS256 and meant for
`JWT_ID_TOKEN_KID` (also with an HMAC `JWT_ALGORITHM`). It needs no database. See
[Signing keys and rotation](oauth-oidc-provider-guide.md#signing-keys-and-rotation).

## Database migrations

By default, startup and database-backed CLI commands apply the embedded
migrations. Set `DB_MIGRATE_ON_START=false` to require an already migrated
database instead; if any embedded migration is missing, initialization fails
with instructions to run `stargate db migrate`. The variable accepts `true`
or `false` and defaults to `true`.

Run migrations explicitly with credentials that can change the schema:

```bash
cargo run --no-default-features --features cluster -- db migrate
```

Use the same database connection variables as the server, or `--features edge`
for SQLite. The command applies migrations regardless of `DB_MIGRATE_ON_START`
and exits without starting the server. For a provisioned PostgreSQL database,
set `POSTGRES_CREATE_DATABASE=false` when the role cannot create databases.

## Export and import configuration documents

These commands use the selected profile's database without starting the HTTP
server, the state store, or the configuration synchronizer. Configuration command
stdout contains only the document or import result, with no banner or application logs.

```bash
cargo run --features edge -- config export --kind gateway --output gateway-backup.yaml
cargo run --features edge -- config export --kind policies > policies-backup
cargo run --features edge -- config import --kind gateway --file gateway.yaml
cargo run --features edge -- config import --kind policies --file policies
```

Use `--features cluster` for PostgreSQL, or run the same arguments with the built
binary (`/bin/server` in the container). Export writes the active document's exact
stored text to stdout or `--output`, including comments and formatting. It exits
unsuccessfully if the document is absent. An empty policies document exports as
empty content.

Import requires an explicit `stargate/v1` gateway schema and parses/compiles the
configuration, or applies the same policy-document analysis as the admin validation
API. It does not read upstream TLS certificates or internal-context keys: those
files need only exist on the receiving server replicas. Invalid input is not saved.

Import reads the current database revision and uses it for the conditional write.
A concurrent revision conflict causes one reread/retry; a second conflict fails
the command. A changed document appends a version and its audit event in the same
transaction (`source: cli_import`, background system actor in the control plane).
Identical content adds neither a version nor an event. The result is exactly
`imported version <n> (<revision>)` or `unchanged`.

Running servers perform full transport/signer preparation before activation.
Cluster replicas react to PostgreSQL notifications and periodic checks. Edge uses
only the timer, so a CLI import from another process sharing the SQLite file is
applied within `CONFIG_SYNC_INTERVAL_SECS` (default 30 seconds), without restarting.
A failed activation keeps the prior snapshot; capacity changes require a restart.
`GET /admin/health` exposes the responding replica's active document revisions.

In Kubernetes edge deployments, run the import inside the existing pod rather
than creating a second pod that mounts the SQLite ReadWriteOnce PVC:

```bash
kubectl exec -n identity deployment/stargate -- /bin/server config import --kind gateway --file /etc/stargate/config.yaml
kubectl exec -n identity deployment/stargate -- /bin/server config import --kind policies --file /etc/stargate/policies
```

The chart's optional `gatewayConfig.importOnUpgrade` hook is cluster-only. See
[Gateway configuration](../k8s/stargate/README.md#gateway-configuration).

## Bootstrap the instance

Use the CLI to create the initial super-admin account and the public OAuth
client required by the Console. Stargate does not auto-create privileged
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

In the container image the binary is `/bin/server`, for example
`docker compose exec stargate /bin/server admin bootstrap ...`.

## Behavior

- `admin bootstrap` creates the first super-admin and the Console OAuth client
  in one database transaction.
- The OAuth client is a public Authorization Code + PKCE client with refresh
  tokens and the `openid`, `email`, `profile`, and `offline_access` scopes.
- `--oauth-client-id` defaults to `CONSOLE_OAUTH_CLIENT_ID`, then
  `stargate_console`.
- `--oauth-redirect-uri` defaults to `CONSOLE_OAUTH_REDIRECT_URI`. When it is
  unset, Stargate derives the callback from `CONSOLE_PUBLIC_URL`,
  `OAUTH_BASE_URL`, or `JWT_ISSUER`, plus `CONSOLE_APP_BASE_PATH`.
- Production redirect URIs must use HTTPS. HTTP is accepted only for loopback
  hosts such as `localhost`.
- Re-running the command leaves matching resources unchanged.
- When a super-admin already exists but the Console OAuth client is missing, run
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

## Runtime requirements

The CLI initializes the database before executing the command, so it uses the
same compiled backend profile and runtime database configuration as the server.

- In `edge` mode, ensure the process can create or access `.stargate/sqlite.db`.
- In `cluster` mode, set the required Postgres environment variables before running the command.
- If `STARGATE_RUNTIME_PROFILE` is set, it must match the compiled profile.
