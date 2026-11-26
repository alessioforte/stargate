# Admin-key permissions

Admin keys are machine credentials for `/admin/*`, with exact capabilities such
as `users:update` or `memberships:create`. They are application-wide; an
organization ID in a request selects a resource, rather than narrowing the key's
scope. Grant only the capabilities an integration needs. Creating and managing
admin keys itself requires an authenticated super-admin user.

## Allowed listeners

An admin key can optionally restrict where it grants authority. Create it with
`POST /admin/admin-keys`, or replace its permissions and listener restrictions
with `PUT /admin/admin-keys/{id}`:

```json
{
  "permissions": ["users:read", "memberships:create"],
  "allowedListeners": ["internal"]
}
```

`allowedListeners: null` allows every listener. The field can be omitted only
when creating a key. In `PUT` it is required: an update containing only
`permissions` is rejected with `400 request.invalid_json` and preserves the
key's listener restrictions. An empty array is rejected, as are invalid names.
Names use the listener configuration's syntax (1–32 ASCII letters or digits,
starting with a letter); whitespace is trimmed, case is normalized to lowercase, and duplicates are removed. The
implicit listener is named `default`.

Creation and updates validate syntax without checking this process's listener
configuration, so a key can be provisioned before a listener is deployed or on a
replica with different listeners. At use, only the actual receiving listener's
name can match; names absent from that process never grant access. A renamed or
removed listener immediately stops matching the key. Matching uses connection
metadata, never request headers, host names, or forwarding headers.

A restricted key on any other listener grants no authority. On `/admin/*` it
receives `401 auth.token_invalid`, with the same body and headers as an unknown
or revoked key, before principal rate limiting, capability checks, or handler
operations. On `/oauth/introspect` and `/oauth/revoke` it follows the same
authentication path as an invalid admin credential, preserving the OAuth error
envelope and headers. The response never reveals whether the key exists.
The server logs the rejection with the key's ID and receiving listener name;
it never logs the key secret. Missing connection metadata fails closed for a
restricted key. Super-admin user credentials retain their existing behavior.

The Console's create and edit drawers accept listener names; leaving the field
empty sends `null`. List, detail, create and update responses include
`allowedListeners`. Creation and changes record the restriction in safe audit
snapshots; updates use `admin_key.updated` with `changed_fields` listing
`permissions` and/or `allowed_listeners` as applicable.

## Protected accounts

Even a key with every capability cannot modify, take control of, or disable a
super-admin account. The server returns `403 user.super_admin_mutation_forbidden`
before changing persistence for these operations:

| Target | Protected operations |
| --- | --- |
| Super-admin user | PUT/PATCH profile (including email/nickname), PUT/PATCH attrs, DELETE |
| Membership of a super-admin | Add, change role, remove |
| Session belonging to a super-admin | Revoke one or all sessions |
| API key owned by a super-admin | Create, delete, revoke, PUT/PATCH attrs |
| Organization containing a super-admin | Update or delete |

Organization changes are protected because password/limit policies affect the
member, and deletion removes memberships and revokes org-bound sessions/keys.
The restriction applies to the whole organization update, including name-only
updates; use a super-admin session to administer these shared organizations.

Super-admin users retain all these operations on their own and other
super-admin accounts through the Console's authenticated OAuth flow or native
sessions, subject to configured MFA/step-up. Deleting a super-admin remains
forbidden to everyone (`user.super_admin_delete_forbidden`).

Read capabilities still allow reading the corresponding resources. Mutations
of normal users, their memberships and keys, service accounts and organizations
without a super-admin continue to use the existing capability checks. No
permission string or wildcard grants super-admin status.

## OAuth clients and other credentials

First-party OAuth clients (`attrs.first_party: true`) are trusted by the admin
OAuth grant. Only a super-admin may create or modify them, change their status,
rotate their secret, or delete them. Admin keys also cannot promote a client to
first-party or remove that flag from a protected client in the same update.
These attempts return `403 auth.insufficient_permissions`. Other clients retain
their capability-based administration.

A user API key is distinct from an admin key: it authenticates as an API-key
subject at the gateway, not as its owner. Owning a key does not grant a user
session, MFA state, or access to `/admin/*`, even for a super-admin owner.

Duplicate canonical emails and nicknames cannot create or invite a replacement
account. Email-shaped login identifiers only resolve emails; a nickname cannot
claim another account's address. Password recovery still requires the signed
reset token sent to the persisted email address; admin keys cannot substitute
their own address or authorize a password change. Public recovery remains
available to the account owner, with its existing email throttling.

Configuration, access-control and OAuth-client capabilities remain sensitive
control-plane permissions. These account protections are not a replacement for
least privilege or review of gateway/client configuration changes.
