# Multi-Org Guide

Users can belong to multiple organizations, each membership with a role
(convention: `owner` | `admin` | `member`). Every user session acts in at
most **one** organization at a time — its *org context* — which drives
per-org access control and org-level rate limits/quotas at the gateway.

Gateway configuration for org-scoped limits:
[`config-v1.md`](config-v1.md) ("Org-scoped limits").

## Which org a session acts in

At login the org context is selected in this order:

1. explicit `orgId` in the `POST /account/login` body (must be one of the
   user's memberships, otherwise the login is rejected with `400`);
2. `users.attrs.default_org`, when still a valid membership;
3. the oldest membership;
4. none — an org-less session.

The selected org is returned as `orgId` in the token response and set as
the `org_id` claim of the access token. Passwordless and social logins use
the same selection, minus the explicit `orgId` step.

## Listing and switching

```
GET /account/organizations
```

returns the caller's memberships with roles and which org the current
session acts in (`activeOrgId`, plus `active` per entry).

```
PUT /account/session/organization
{ "orgId": "01H8XYZ..." }
```

issues a **fresh token pair** (and session cookie) acting in the requested
org. Switching *forks*: the previous tokens keep working with their
original org context until they expire, so a client can hold parallel
sessions in different orgs. Each forked session has its own refresh chain.

The hosted login UI shows an organization picker after sign-in whenever the
user belongs to more than one org.

## Sessions ending

- `DELETE /account/logout` — ends the current session.
- `POST /account/logout/all` — ends **every** session of the user,
  including forked org sessions on other devices; access and refresh tokens
  stop working immediately.
- Removing a user from an org immediately ends their sessions acting in
  that org (org-less and other-org sessions survive) and **revokes** their
  API keys bound to that org.
- Deleting a user or an organization revokes all affected sessions and API
  keys immediately.
- Refresh re-validates the membership of the session's active org: a
  removed member cannot carry the org context past the access-token
  lifetime even if event-driven revocation were missed.

## Membership administration

```
PUT    /admin/users/{id}/organizations/{org_id}   { "role": "member" }   # upsert (add or change role)
DELETE /admin/users/{id}/organizations/{org_id}                          # remove + revoke org-bound keys/sessions
GET    /admin/users/{id}/organizations                                   # memberships with role + memberSince
GET    /admin/users/organizations/{org_id}                               # members with role + memberSince
```

The body on `PUT` is optional; omitting it defaults the role to `member`.
Roles are free-form strings (max 50 chars); the Console offers the
conventional `owner`/`admin`/`member`.

### Provisioning a user with an initial membership

The existing direct-create and invitation endpoints accept the same optional
membership object:

```json
{
  "email": "admin@tenant.example",
  "password": "use-a-policy-compliant-secret",
  "membership": {
    "organizationId": "01H8XYZ...",
    "role": "admin"
  }
}
```

Omit `password` when sending the body to `POST /admin/users/invitations`.
Omitting `membership` preserves the original organization-less behavior. A
machine principal needs `users:create` or `users:invite`, plus
`memberships:create` when the object is present. Admin keys are
application-wide: the organization ID selects the membership to create, not a
scope attached to the key.

Direct creation commits the user, credential, membership, and audit outbox
events in one transaction. Invitations retain the normalized organization and
role in server-side pending state and use the same transaction when accepted.

## API keys and orgs

- User API keys may carry an optional org binding (`orgId` at creation;
  the owner must be a member). A bound key always acts in that org.
- Service-account keys inherit the service account's org.
- Bindings are revoked-with-the-membership: leaving the org, deleting the
  org, or deleting the owner revokes the keys rather than degrading them
  to org-less credentials.

## What the org context feeds

- **ACE policies**: `user.org_id`, `user.org_role`, `api_key.org_id`
  condition variables (see the ACE section in `config-v1.md`).
- **Gateway limits**: `scope: org` rate limits and quotas consume the
  org's shared bucket; orgs override the named limit via
  `organizations.attrs.rate_limit` / `attrs.quota`.
