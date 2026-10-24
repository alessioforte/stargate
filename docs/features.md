# Features

Some capabilities are switched on or off per deployment with environment
variables. Stargate reads them once at startup, so a change needs a restart.
An invalid value, or a combination that cannot work, stops startup with a
message that names the variable. The active features are logged at startup:

```text
Features: signup=invite passwordless=off console=on social=[github]
```

| Variable | Default | Controls |
| --- | --- | --- |
| `SIGNUP_MODE` | `invite` | Who may create accounts: `open`, `invite`, or `closed` |
| `SIGNUP_ALLOWED_DOMAINS` | none | Email domains self-registration is limited to (`open` only) |
| `PASSWORDLESS_LOGIN_ENABLED` | `false` | Signing in with an emailed code instead of a password |
| `CONSOLE_ENABLED` | `true` | Serving the Console app |
| `GOOGLE_OAUTH_CLIENT_ID` + `GOOGLE_OAUTH_CLIENT_SECRET` | unset | Google sign-in, on when both are set |
| `GITHUB_OAUTH_CLIENT_ID` + `GITHUB_OAUTH_CLIENT_SECRET` | unset | GitHub sign-in, on when both are set |

Booleans accept `true`/`false`, `1`/`0`, `yes`/`no`, and `on`/`off`, in any
case; anything else is an error rather than `false`.

## How a disabled feature answers

A disabled feature keeps its routes, which answer before reading the request:

```json
{
  "code": "feature.disabled",
  "message": "This feature is disabled on this server",
  "params": { "feature": "signup" }
}
```

with status `404`. `params.feature` is `signup`, `invitations`,
`passwordless_login`, `console`, `social` (`/oauth/state` with no provider),
`social_google`, or `social_github`. The routes stay mounted on purpose:
paths Stargate does not serve fall through to the gateway, which could proxy
them to an upstream.

To discover what is on:

- `GET /account/features` (public, cacheable for a minute), used by the
  hosted login app:

  ```json
  { "signupMode": "invite", "passwordlessLogin": false, "socialProviders": ["github"] }
  ```

- `GET /admin/health` (administrators) adds `signupAllowedDomains` and
  `console`; the Console dashboard shows them in its health card.

## Account creation

`SIGNUP_MODE` decides who may create accounts. Administrators always can,
with `POST /admin/users` (or the Console).

| | `open` | `invite` (default) | `closed` |
| --- | --- | --- | --- |
| Self-registration, `POST /signup` | yes | `404` (`signup`) | `404` (`signup`) |
| First social sign-in, email without an account | creates the account | `403 social.account_not_provisioned` | `403 social.account_not_provisioned` |
| Social sign-in to an existing account | yes | yes | yes |
| Admin invitations, `POST /admin/users/invitations` | yes | yes | `404` (`invitations`) |
| Completing a signup from the emailed link, `GET`/`PUT /signup` | yes | invitations only | `404` (`invitations`) |

- **`SIGNUP_ALLOWED_DOMAINS`** limits self-registration and accounts created
  by a first social sign-in to these domains: comma-separated, matched
  exactly against the email's domain (`example.com` does not cover
  `sub.example.com`; no wildcards). Others get
  `403 signup.domain_not_allowed`. Admin invitations and creations are not
  limited. Setting it with a mode other than `open` stops startup.
- **A stricter mode applies to signups under way.** A pending signup records
  whether it started as a self-registration or an invitation; a
  self-registration completes only while the mode is still `open` and its
  domain still allowed.
- `POST /signup` still answers the same way whether or not the address has an
  account; the domain check depends on the address only.
- The hosted login page links to sign-up only in `open` mode, and `/auth/signup`
  without a link explains that accounts come from an administrator.

## Passwordless login

`PASSWORDLESS_LOGIN_ENABLED=true` turns on `POST`/`PUT
/account/login/otp/email`: the user enters an email address and signs in with
the code sent to it. The hosted login page then shows "Sign in with email
code".

- It needs `EMAIL_OTP_PEPPER`, which secures the codes; without it startup
  stops.
- With `MFA_MODE=required` startup stops: a code alone would never be enough.
- Users who need MFA (`MFA_MODE=optional` with MFA enabled, or super-admins
  when `MFA_REQUIRED_FOR_SUPER_ADMIN=true`) still cannot use it.

## Social sign-in

A provider is on when both its client id and secret are set; one without the
other stops startup. Without credentials its route answers `404` instead of
an upstream error.

- Request the `user:email` scope from GitHub and `email` (plus `profile` for
  the name and picture) from Google: accounts are matched by an email the
  provider verified, otherwise `403 social.email_unverified`.
- Whether a first sign-in creates the account follows `SIGNUP_MODE` (see
  above).

## Console

`CONSOLE_ENABLED=false` stops serving the Console app: its path
(`CONSOLE_APP_BASE_PATH`, default `/stargate`) answers `404`. The admin API
(`/admin/*`) and the Console's OAuth client stay, so a Console hosted
elsewhere (`CONSOLE_PUBLIC_URL`) keeps working.

## What is not switchable

OpenID Connect provider capabilities (`response_mode=form_post`,
`private_key_jwt`, consent, back-channel logout, `acr_values`) have no global
switches: each is opted into per client, and a server without such clients
does not use them. The Console signs in through the provider, so the provider
itself cannot be turned off.

## Examples

Workforce or internal use: administrators create accounts, nothing public.

```env
SIGNUP_MODE=closed
MFA_MODE=required
# On public nodes; serve the Console from an internal one.
CONSOLE_ENABLED=false
```

B2B: invitations, or open registration for the company's domains.

```env
SIGNUP_MODE=open
SIGNUP_ALLOWED_DOMAINS=example.com,example.org
```

Consumer apps: anyone can register, with social and passwordless sign-in.

```env
SIGNUP_MODE=open
PASSWORDLESS_LOGIN_ENABLED=true
# openssl rand -hex 32
EMAIL_OTP_PEPPER=...
GITHUB_OAUTH_CLIENT_ID=...
GITHUB_OAUTH_CLIENT_SECRET=...
```

Edge gateway for machine clients (API keys, `client_credentials`): no
end-user sign-up.

```env
SIGNUP_MODE=closed
```

## Kubernetes

With the [Helm chart](../k8s/stargate/README.md#features), put the switches in
`runtimeConfig.data` and the secrets (`EMAIL_OTP_PEPPER`, the OAuth client
secrets) in `runtimeSecret`.
