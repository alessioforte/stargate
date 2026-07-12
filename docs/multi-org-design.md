# Multi-Org Users: Sessions, Access Control, and Org-Level Limits

**Status: accepted — not yet implemented.**
Drafted 2026-07-12; decisions resolved 2026-07-12 (see end).
Task breakdown: [`multi-org-tasks.md`](multi-org-tasks.md).

This document designs three related features:

1. Users belonging to multiple organizations, with a per-membership role.
2. An org context in user sessions ("which org am I acting in right now"),
   including how to switch it.
3. Org-scoped access control (ACE) and org-level rate limits / quota
   trackers in the gateway.

## Starting point

The codebase is closer than it looks, with three latent gaps:

- **Multi-org membership already exists structurally.** `user_organizations`
  is an M:N join table
  (`migrations/postgres/20260613000000_init_schema.sql`) with admin
  add/remove endpoints under `/admin/users/{id}/organizations/{org_id}`. It
  has no `role` column — membership is binary.
- **`Subject.org_id` exists but is never populated** (`src/etc/sub.rs`).
  ACE never sees an org, and limits never key on one.
- **Sessions are already server-side**, which is the key asset for the
  session question: login stores a serialized `Subject` under `sid` in the
  store (`src/api/account/session.rs`), the JWT only carries `sid`, and the
  gateway resolves the subject by looking up `sid`
  (`src/etc/guard.rs::verify_jwt`). Org context can therefore live in the
  session record, not the token format — switching orgs is a store write,
  not a token redesign.

## 1. Session model (the core decision)

Three viable models:

| | A. Active org in session | B. Org per request (header) | C. Org-scoped access tokens |
|---|---|---|---|
| How | `sid → Subject` gains `org_id`/`org_role`; switch = rewrite + re-mint | Client sends `x-org-id`; gateway validates membership per request | Refresh token is org-agnostic; client mints one access token per org |
| Parallel orgs (multi-tab) | No (one context per session) | Yes | Yes |
| Hot-path cost | Zero extra lookups | Membership check per request | Per-org sid records, or trusting the JWT claim |
| Fits current architecture | Perfectly | Fights the store-based subject | Needs gateway changes to subject resolution |

**Decision: model A, with fork-on-switch semantics that yield model C's
behavior for free** (see below).

### Login

After password/MFA succeeds, pick the active org:

1. Single membership → that org.
2. Multiple memberships → `users.attrs.default_org` if set, else last-used,
   else first.
3. `POST /account/login` accepts an optional `orgId` to pick the initial
   context explicitly; the `AuthResponse` gains an `orgId` field.

Populate `Subject.org_id` and `Subject.org_role` before storing under `sid`,
and add an `org_id` claim to the access token. The claim is informational
for clients (and, optionally, OIDC) — the gateway keeps trusting the store,
not the claim.

### Switching orgs

`PUT /account/session/organization` with `{ "orgId": "..." }`:

1. Validate the caller's membership in the target org.
2. Mint a **new sid + new token pair** carrying the new org context.
3. Store the new `Subject` under the new sid.

**The knob — what happens to the old sid:**

- **Replace** (delete the old sid): strict single-org-context per login.
  Old access tokens die immediately.
- **Fork** (keep the old sid until its TTL): old tokens keep working *with
  their old org context*. Every token pair is org-consistent, and a client
  holding two pairs effectively has two org contexts in parallel — model C's
  behavior without touching the gateway. Each fork gets its own refresh
  chain, since refresh already rotates sid per chain
  (`src/api/account/refresh_token.rs`).

**Recommendation: fork.** It is less code (nothing to delete), the more
useful semantics, and tokens never lie about their org. Forking makes
"logout everywhere" and admin revocation need to enumerate a user's sids,
so add a `user:sessions:{user_id}` set in the store maintained at
login/switch/refresh/logout. Nothing indexes sessions by user today.

### Refresh

The refresh flow already re-reads the user from the DB. Add a membership
re-check on the session's active org there:

- Still a member → carry the org context (and re-resolve `org_role`) into
  the new sid.
- Removed from the org → reject the refresh (or downgrade to an org-less
  session; rejecting is simpler and safer).

This is a backstop: primary revocation is active — membership removal and
user deletion delete the affected sid records directly via the session
index (see Resolved decisions), so tokens die immediately without any
hot-path membership check.

### API keys

API keys are the other subject type and need an org story:

- **Service-account keys**: `service_accounts.org_id` already exists.
  Populate `Subject.org_id` from it in `verify_api_key`
  (`src/etc/guard.rs`). One extra join per store-cache fill (subjects are
  cached for an hour), not per request.
- **User keys**: add a nullable `org_id` column to `user_api_keys` (FK to
  `organizations`, validated as an actual membership at key-creation time).
  A key bound to an org always acts in that org; an unbound key acts
  org-less.
- **Binding lifecycle**: the binding is not re-checked at auth time —
  revocation happens at the event, not per request. Removing the user from
  the org (and, pending validation, deleting the org) **revokes** the
  bound keys rather than unbinding them, since unbinding would silently
  grant org-less access. Note: deleting a user or service account today
  orphans its `api_keys` rows, which keep authenticating (only the link
  rows cascade) — phase 2 closes this by revoking keys in the delete
  flows.

## 2. Schema changes

```sql
-- user_organizations: membership becomes role-carrying
ALTER TABLE user_organizations ADD COLUMN role TEXT NOT NULL DEFAULT 'member';
ALTER TABLE user_organizations ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

-- user API keys get optional org binding
ALTER TABLE user_api_keys ADD COLUMN org_id TEXT REFERENCES organizations(id) ON DELETE SET NULL;
```

(SQLite equivalents mirror these; `created_at` defaults per backend.)

- **Roles** are free-form strings with a conventional vocabulary
  (`owner` / `admin` / `member`) rather than a DB enum — ACE conditions
  compare strings anyway, and this avoids a migration per new role.
- **Default org preference** lives in `users.attrs.default_org` (no schema
  change), following the existing attrs-override precedent
  (`attrs.password_policy`, `attrs.mfa`).
- **`Subject`** gains `org_role: Option<String>` next to the existing
  `org_id`. Old serialized sessions deserialize fine via serde defaults.

## 3. Per-org access control (ACE)

Two changes in `src/etc/ac.rs`; the second is a correctness prerequisite,
not a follow-up.

### Context injection

`create_context` currently injects only subject attrs (namespaced by
subject type) and `env.*`. Add:

- `user.org_id`, `user.org_role` for user subjects;
- `api_key.org_id` for key subjects;
- optionally an `org.*` namespace fed from the cached org-attrs blob
  (§4), enabling org-tier conditions.

Policy examples:

```text
ALLOW user FOR "reports:read" WHEN user.org_role == "admin" OR user.org_role == "owner";
ALLOW user FOR "billing" WHEN user.org_id == "01H..." AND user.org_role == "owner";
ALLOW api_key FOR "export" WHEN org.plan == "enterprise";
```

### Decision-cache key fix (trap)

The thread-local decision cache key (`DecisionKey` in `src/etc/ac.rs`) is
`sub_type + resource + action + attrs_hash` — it does not include the org.
Two users in different orgs with identical attrs share a cache entry today,
and after an org switch the same user would keep hitting their pre-switch
decision. `org_id` and `org_role` **must** join the key (either as fields
or folded into the hash). Land this in the same commit as context
injection.

### Resource naming

Keep resource names global (`reports:read`) and express org scoping in
conditions. Per-org resource names would explode the policy space and
defeat the decision cache.

## 4. Org-level rate limits and quotas

Today (`src/api/gateway/limits.rs`): one rate check + one quota check per
request, keyed `lim:{subject_id}` / `quota:{subject_id}`, with the limit
name resolved policy-override → subject attrs (`rate_limit` / `quota`) →
`"default"`. `apply_policies` (`src/api/gateway/policies.rs`) rejects
multiple rate_limit or quota policies on one router.

**Design: add a `scope` dimension to limit policies, and relax "one policy
per router" to "one per scope".**

### Config (v2alpha1)

```yaml
http:
  policies:
    api-rate:
      rate_limit:
        limit: default
        scope: subject        # default — existing behavior
    org-rate:
      rate_limit:
        limit: org-default
        scope: org
        on_missing: skip      # skip | ip_fallback | deny — when subject has no org
    org-quota:
      quota:
        limit: org-monthly
        scope: org
        cost: 1

  routers:
    api:
      # subject AND org limits coexist on one router
      policies: [require-auth, api-rate, org-rate, org-quota]
```

### Semantics

- **Keys**: `lim:org:{org_id}` and `quota:org:{org_id}`. In cluster mode
  limiter state is Redis-backed, so org counters are shared across nodes;
  edge is per-process, same as today's subject limits.
- **Per-org overrides**: mirror the subject-attrs precedent —
  `organizations.attrs.rate_limit` / `organizations.attrs.quota` name a
  configured limit spec and override the policy's default name for
  `scope: org` checks. An enterprise org gets
  `attrs: { rate_limit: "org-premium" }` and nothing else changes.
- **Ordering**: run the org check before the subject check (coarser gate
  first).
- **Double-charge caveat**: with consume-on-check strategies (GCRA), if the
  first check consumes and a later check denies, the first bucket was
  charged for a rejected request. This is accepted imprecision (typical for
  gateways). If it ever matters, add a refund/peek operation to `lim`
  strategies — do not build it now.
- **Headers**: when the org limit is the binding constraint, report it in
  `x-ratelimit-*` / `x-quota-*`. Consider `x-ratelimit-scope: org|subject`
  so clients can tell which bucket they hit.

### Org attrs on the hot path

Do **not** embed org attrs in the session (stale until re-login). Instead:

- Cache `org:ctx:{org_id}` → `{ rate_limit, quota, plan, … }` in the store
  with a short TTL (~60s).
- The admin org-update handler deletes the cache key on write, so changes
  propagate within one request, not one TTL.
- Cost: one store GET per request on routes with org-scoped policies (a
  Redis GET in cluster). If benchmarks flag it, wrap in a small in-process
  TTL cache (thread-local LRUs are already an established pattern here).
- The same cached blob feeds the `org.*` ACE namespace (§3).

## 5. API surface

Account:

- `GET /account/organizations` — the caller's memberships with roles, plus
  which org is active in this session.
- `PUT /account/session/organization` `{ "orgId": ... }` — switch active
  org; returns a fresh `AuthResponse`.
- `POST /account/login` — optional `orgId` in the body; `orgId` in the
  response.

Admin:

- `PUT /admin/users/{id}/organizations/{org_id}` — already exists for add;
  give it a body `{ "role": "member" }` and make it upsert
  (add-or-change-role).
- Org limit/plan management is the existing org-attrs update endpoint — no
  new API.

## 6. Rollout order

Each phase ships independently with no behavior change until its
config/policies are used:

1. **Schema + plumbing**: role column, `user_api_keys.org_id`, populate
   `Subject.org_id`/`org_role` at login and in `verify_api_key`, add the
   `org_id` JWT claim. Nothing reads any of it yet.
2. **Session**: default-org selection at login, switch endpoint (fork
   semantics), membership re-check on refresh, `user:sessions:{user_id}`
   index, `GET /account/organizations`.
3. **ACE**: org context injection + decision-cache key fix, in the same
   commit.
4. **Limits**: `scope` on rate_limit/quota policies, multi-scope
   `apply_limits`, `org:ctx` cache + admin invalidation.
5. **UI**: org switcher, membership/role management, org limit settings.

## Resolved decisions

1. **Fork vs replace on switch** → **fork.** Old sids stay alive until
   their TTL; each token pair is org-consistent; the
   `user:sessions:{user_id}` index (a store hash of sid → org context)
   makes multi-sid revocation possible.
2. **OIDC exposure** → **internal only, for now.** The `org_id` claim goes
   into session access tokens; ID tokens and `/oauth/userinfo` are
   unchanged. Revisit if OAuth clients need org context.
3. **Revocation** → **active, immediate.** Deleting a user deletes all of
   their sid records via the session index; removing a user from an org
   deletes the sessions whose active org is that org. Because every
   authenticated request resolves the subject through the store by `sid`,
   affected JWTs stop working immediately — the signature remains valid
   until `exp`, but no session resolves behind it. The refresh-time
   membership re-check remains as a backstop. Note this is a behavior
   change from today, where a deleted user's access token keeps working
   until it expires (nothing indexes sessions by user yet).
