# Multi-Org: Implementation Task List

Task breakdown for [`multi-org-design.md`](multi-org-design.md).
Decisions locked: **fork on switch**, **`org_id` internal to session
tokens only**, **active session revocation on user delete / membership
removal**.

Each phase is independently shippable with no behavior change until its
config or endpoints are used. Items marked **validate:** are micro-details
to confirm or change before that phase starts.

Sizes: S = hours, M = ~a day, L = multiple days.

---

## Phase 1 — Schema + subject plumbing (no behavior change)

Goal: the data model knows about roles and org bindings; subjects carry
org context where it is unambiguous. Nothing reads it yet.

- [x] **Migration** (new file in `migrations/postgres/` and
  `migrations/sqlite/`): (S)
  - `user_organizations` + `role TEXT NOT NULL DEFAULT 'member'`
  - `user_organizations` + `created_at` (backend-appropriate default)
  - `user_api_keys` + `org_id TEXT REFERENCES organizations(id)
    ON DELETE SET NULL` (nullable)
  - **validate:** role vocabulary is free-form with convention
    `owner` / `admin` / `member`, default `member` — no DB enum.
- [x] **`crates/db` repo layer**: (M)
  - `get_user_organizations` returns role alongside each org (new
    membership struct or extended row).
  - New `get_user_organization(user_id, org_id) -> Option<Membership>`
    for cheap single-membership checks (login, switch, refresh).
  - `add_user_to_organization(user_id, org_id, role, ctx)` becomes an
    upsert (insert or update role).
  - User API key creation accepts optional `org_id`; key lookup joins the
    binding. Service-account key lookup joins `service_accounts.org_id`.
- [x] **`Subject`** (`src/etc/sub.rs`): add `org_role: Option<String>`
  (serde-default so existing serialized sessions keep deserializing). (S)
- [x] **`verify_api_key`** (`src/etc/guard.rs`): populate `org_id` on the
  subject — from `service_accounts.org_id` for SA keys, from
  `user_api_keys.org_id` for user keys. One extra join per store-cache
  fill (1h TTL), not per request. (S)
- [x] **`crates/jwt`**: add optional `org_id` claim + builder method. Field
  only — nothing sets it until phase 2. (S)
- [x] **Admin API**: (M)
  - `PUT /admin/users/{id}/organizations/{org_id}` accepts optional body
    `{ "role": "member" }`, upserts (add-or-change-role).
  - `GET /admin/users/{id}/organizations` and
    `GET /admin/users/organizations/{org_id}` include `role`.
  - User API key creation endpoints accept `orgId`; reject if the owner
    is not a member of that org.
- [x] **Tests**: repo upsert/membership queries, subject serde
  backward-compat, key-creation org validation. (S)

**Done when:** existing tests pass untouched; new columns and fields are
readable end-to-end; runtime behavior is byte-identical for current
clients.

---

## Phase 2 — Session org context, fork switching, active revocation

Goal: sessions carry an active org; users can switch (fork); removing a
user or a membership kills the affected sessions immediately.

- [x] **Session index** (new module, e.g. `src/act/sessions.rs`): (M)
  - Store hash `user:sessions:{user_id}`: field = `sid`, value =
    `{ org_id, expires_at }` (store already has `hset`/`hgetall`/`hdel`).
  - Written at login and switch; refresh moves the entry (hdel old sid,
    hset new); logout hdels.
  - Hash TTL refreshed to `JWT_REFRESH_EXP` on write; expired fields
    pruned lazily on read.
- [x] **Default-org selection at login** (`src/api/account/session.rs`): (M)
  - Order: explicit `orgId` in the login body (must be a membership) →
    `users.attrs.default_org` (if still a member) → oldest membership
    (`created_at`) → none (org-less session).
  - Populate `Subject.org_id`/`org_role`, set the `org_id` JWT claim, add
    `orgId` to `AuthResponse`.
  - **validate:** no "last-used org" tracking for now (switch does not
    write `users.attrs`); `default_org` is the only stickiness.
- [x] **Switch endpoint** `PUT /account/session/organization`
  (`src/api/account/`): validate membership → mint new sid + token pair
  with new org context → store new Subject + index entry. Fork: the old
  sid is left untouched and expires on its own TTL. (M)
- [x] **`GET /account/organizations`**: caller's memberships with roles,
  plus which org is active in the current session. (S)
- [x] **Refresh re-check** (`src/api/account/refresh_token.rs`): the flow
  already re-reads the user; also re-check membership of the session's
  active org. Missing → reject with 401 (no downgrade). Still a member →
  carry org forward with a freshly-read role. Update the session index for
  the sid rotation. (S)
  - **validate:** reject vs downgrade-to-org-less. Design says reject.
- [x] **Revocation on user delete** (`src/api/admin/users.rs::delete_user`):
  after the DB delete: (M)
  - delete every sid in `user:sessions:{id}`, then the index key;
  - **revoke (or delete) the user's `api_keys` rows in the same
    transaction** — the FK cascade only removes the `user_api_keys` link
    rows; the `api_keys` rows survive orphaned and keep authenticating
    (pre-existing gap, discovered 2026-07-12; also affects
    service-account deletion);
  - delete the store cache entry for each of those keys (`key_hash` —
    precedent: `revoke_api_key` in `src/api/admin/api_keys.rs`), since
    cached subjects would otherwise live up to 1h.
- [x] **Revocation on membership removal**
  (`src/api/admin/users.rs::remove_user_from_organization`): (M)
  - `hgetall` the index; delete sids whose `org_id` is the removed org
    (org-less and other-org sessions survive);
  - user API keys bound to that org get **revoked** (not unbound) —
    unbinding would silently grant org-less access. *(validated
    2026-07-12)*
- [x] **Revocation on org delete**
  (`src/api/admin/organizations.rs` delete handler): same "revoke, don't
  unbind" logic — revoke user keys bound to the deleted org (the FK
  `ON DELETE SET NULL` stays as a backstop but must not be the only
  mechanism), revoke keys of the org's cascaded service accounts, kill
  sessions active in that org, and clear affected store caches. (M)
  - **validate:** org deletion revokes org-bound user keys instead of
    degrading them to org-less keys. Confirm.
- [x] **Fix orphaned-key authentication** independent of multi-org:
  service-account deletion also orphans its `api_keys` rows today. Either
  revoke keys in `delete_service_account`, or make `verify_api_key`
  reject keys with no owner link. (S)
- [x] **Logout**: hdel the sid from the index. Optional follow-up now
  trivial: `POST /account/logout/all` iterating the index. (S)
  - **validate:** include logout-all in this phase or defer to phase 5?
- [x] **Tests**: fork produces two independently working token pairs with
  distinct org contexts and independent refresh chains; user delete kills
  all tokens on the next request; membership removal kills only that
  org's sessions; refresh dies after membership removal. (M)

**Done when:** the revocation acceptance tests above pass; a user with no
orgs still logs in and behaves exactly as today.

---

## Phase 3 — ACE per-org (one commit)

Goal: policies can condition on org. The decision-cache fix and the
context injection land **in the same commit** — the cache key without org
is a cross-org correctness bug the moment org context exists.

- [ ] **Context injection** (`src/etc/ac.rs::create_context`): add
  `user.org_id`, `user.org_role` for user subjects and `api_key.org_id`
  for key subjects (skip when `None`). (S)
- [ ] **Decision-cache key** (`src/etc/ac.rs::DecisionKey`): include
  `org_id` and `org_role` (fields or folded into the hash). (S)
- [ ] **Tests**: same subject attrs, different orgs → independent cache
  entries and decisions; org switch changes the decision without a policy
  reload; org-less subjects unaffected. (S)
- [ ] **Docs**: policy examples in the design doc / ACE docs:
  ```text
  ALLOW user FOR "reports:read" WHEN user.org_role == "admin" OR user.org_role == "owner";
  ALLOW user FOR "billing" WHEN user.org_id == "01H..." AND user.org_role == "owner";
  ```
  (S)

**Done when:** org-conditioned policies enforce correctly across org
switches; resource names stay global (org scoping only in conditions).

---

## Phase 4 — Org-scoped rate limits and quotas

Goal: routes can enforce subject **and** org limits side by side; orgs
override limit names via attrs; changes propagate without re-login.

- [ ] **`crates/gate` v2alpha1 schema**
  (`crates/gate/src/cfg/v2alpha1.rs`, `graph.rs`): `rate_limit` and
  `quota` policies gain `scope: subject | org` (default `subject`) and
  `on_missing: skip | ip_fallback | deny` (default `skip`) for
  `scope: org` when the subject has no org. Compile into `PolicyNode`;
  validation rejects unknown values. (M)
  - **validate:** `on_missing` default `skip` — an org-less subject passes
    the org policy silently. Alternative default `deny` for org-only APIs.
- [ ] **Policy collection** (`src/api/gateway/policies.rs`): relax
  "multiple rate_limit/quota policies unsupported" to **one per scope**;
  duplicates within a scope remain an error. (S)
- [ ] **Org context cache**: `org:ctx:{org_id}` in the store, TTL ~60s,
  holding the org-attrs subset the gateway needs
  (`rate_limit`, `quota`, `plan`, …). Loader on the gateway path;
  admin org-update handler deletes the key on write so changes propagate
  within one request. (M)
- [ ] **`apply_limits`** (`src/api/gateway/limits.rs`): (L)
  - Multi-scope: run org checks first (coarser gate), then subject.
  - Org keys: `lim:org:{org_id}` / `quota:org:{org_id}`.
  - Limit-name resolution for `scope: org`: policy name → org attrs
    override (from `org:ctx` cache) — mirroring the existing subject-attrs
    precedence.
  - Headers report the binding constraint; add `x-ratelimit-scope:
    org | subject`.
  - **validate:** the `x-ratelimit-scope` header (new contract surface).
  - Known accepted imprecision: consume-on-check means an earlier bucket
    may be charged when a later one denies (documented; refund op in
    `crates/lim` only if it ever matters).
- [ ] **Optional `org.*` ACE namespace**: feed `create_context` from the
  same `org:ctx` cache so policies can use `org.plan == "enterprise"`.
  If included, the ACE decision-cache key must also include the org-ctx
  version/hash. (M)
  - **validate:** include now or defer? Deferring keeps phase 4 purely
    about limits.
- [ ] **Docs**: `docs/config-v2alpha1.md` — `scope`, `on_missing`, org
  attr overrides, header semantics. (S)
- [ ] **Tests**: two orgs on one route consume separate org buckets;
  subject + org limits coexist and the stricter one binds; org attr
  change takes effect on the next request after admin update; edge
  (in-process) and cluster (Redis) parity. (M)

**Done when:** the config below works as written:

```yaml
http:
  policies:
    api-rate:  { rate_limit: { limit: default,     scope: subject } }
    org-rate:  { rate_limit: { limit: org-default, scope: org, on_missing: skip } }
    org-quota: { quota:      { limit: org-monthly, scope: org, cost: 1 } }
  routers:
    api:
      policies: [require-auth, api-rate, org-rate, org-quota]
```

---

## Phase 5 — UI and polish

- [ ] Org switcher in the UI (list from `GET /account/organizations`,
  switch via `PUT /account/session/organization`, swap tokens client-side).
- [ ] Admin UI: membership role management (upsert role), org limit/plan
  attrs editing.
- [ ] API key creation UI: optional org binding for user keys.
- [ ] `POST /account/logout/all` if deferred from phase 2.
- [ ] User-facing docs: org switching guide; update
  `docs/multi-org-design.md` status to "implemented".

---

## Summary of validate: items

| Phase | Item | Proposed default |
|-------|------|------------------|
| 1 | Role vocabulary | free-form strings; `owner`/`admin`/`member` convention, default `member` |
| 2 | Last-used org tracking | none — only `users.attrs.default_org` |
| 2 | Refresh when membership removed | reject (401), no downgrade |
| 2 | Org-bound user API keys on membership removal | revoke, don't unbind *(confirmed)* |
| 2 | Org-bound user API keys on **org deletion** | revoke, don't degrade to org-less |
| 2 | `logout/all` endpoint | defer to phase 5 |
| 4 | `on_missing` default | `skip` |
| 4 | `x-ratelimit-scope` header | add it |
| 4 | `org.*` ACE namespace | defer decision to phase 4 start |
