# Stargate CLI

Stargate includes a built-in CLI for administrative tasks. The server checks CLI
arguments first, and when a command is provided it runs the CLI flow instead of
starting the HTTP server.

Current command tree:

```text
stargate admin bootstrap
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
