# Redis state keys

Every key Stargate writes to Redis or to the edge in-memory store follows one
schema:

```text
[REDIS_KEY_PREFIX]stargate:v1:{<entity>}:<name>[:<qualifier>...]
```

- **Namespace and version** — `stargate:v1:`. The version is part of every
  key, so a schema change starts from empty sessions and counters instead of
  reading incompatible state. This schema is the first version: deploying it
  leaves the older unnamespaced keys unread, so every user signs in again and
  limit counters restart from zero. No migration is needed; the old keys
  expire on their own TTL or can be deleted with the previous ACL pattern.
- **Entity hash tag** — `{<entity>}`. Redis Cluster assigns a slot from the
  first `{...}` pair. All keys used together in one command, Lua script or
  transaction carry the same tag, so they land in the same slot and never
  produce `CROSSSLOT` or `non local key`.
- **Name and qualifiers** — what the key stores. A qualifier may contain
  separators and limit names; it never contains braces.

`REDIS_KEY_PREFIX` (default empty) is the operator's literal prefix, applied
before the schema root. Use it to share one Redis instance between
deployments; changing it hides all state of the previous namespace. It must not
contain `{` or `}`: Redis hashes the first brace pair of the physical key, so a
prefix such as `{stargate}:` would put every key in one Cluster slot. Startup
rejects such a prefix.

The tag is built from generated identifiers (ULIDs, digests) or an IP address.
Free-form values are hashed with SHA-256: emails, login identifiers and OTP
subjects never appear in a key or a tag.

## Keys

| Key | Stores | TTL |
|:--|:--|:--|
| `{s:<sid>}:subject` | Signed-in subject of a session | refresh-token TTL |
| `{s:<sid>}:refresh` | Current native refresh-token `jti` | refresh-token TTL |
| `{u:<user_id>}:sessions` | The user's session index (sid → org, expiry) | per-field TTL |
| `{g}:sessions` | Global session directory (sid → session record) | per-field TTL |
| `{j:<jti>}:revoked` | A revoked JWT `jti` | token expiry |
| `{k:<key hash>}:subject` | Cached API-key subject | 300 s |
| `{c:<code hash>}:code` | Pending or redeemed authorization code | code TTL |
| `{f:<family id>}:refresh` | Rotating OAuth refresh-token family | refresh-token TTL |
| `{i:<id>}:authorize` | Parked authorization request | 10 min |
| `{i:<id>}:logout` | Parked RP-initiated logout request | 10 min |
| `{o:<state>}:state` | Social sign-in state consumed with `GETDEL` | 5 min |
| `{a:<client id>}:assertion:<jti hash>` | Accepted `private_key_jwt` assertion | assertion expiry |
| `{otp:<challenge>}:passwordless` | Passwordless email login challenge | OTP TTL |
| `{otp:<challenge>}:login` | MFA challenge of a pending login | OTP TTL |
| `{u:<user_id>}:mfa:<challenge>` | Authenticated MFA challenge | OTP TTL |
| `{s:<sid>}:mfa:<purpose>` | Step-up marker verified in a session | step-up TTL |
| `{ip:<ip>}:otp-request` | OTP requests from one address | 5 min |
| `{otp:<subject hash>}:requests:<ip>` | OTP requests for one subject and address | 5 min |
| `{em:<email hash>}:password-reset` | Pending password reset of an address | 1 h |
| `{v:<sid>}:verification` | Pending email verification request | 1 h |
| `{em:<email hash>}:verification-lookup` | Address → verification request id | 1 h |
| `{sg:<sid>}:signup` | Sign-up copy created after verification | 1 h |
| `{em:<email hash>}:email:<flow>` | Per-recipient email throttle | 1 h |
| `{ip:<ip>}:login-attempts` | Reserved password attempts from one address | 5 min from the first reservation |
| `{u:<user_id>}:login-attempts:<ip>` | Reserved password attempts against a user from one address | 5 min from the first reservation |
| `{id:<identifier hash>}:login-attempts:<ip>` | Reserved password attempts for an unknown identifier from one address | 5 min from the first reservation |
| `{u:<user_id>}:login-known` | Recognized addresses of completed password logins (address → deadline) | per-field TTL, 30 days renewed on completed password login |
| `{u:<user_id>}:login-attempts` | Account-wide password attempts and backoff timestamp | 5 min from the last reservation |
| `{id:<identifier hash>}:login-attempts` | Password attempts and backoff for an unknown identifier across addresses | 5 min from the last reservation |
| `{ip:<ip>}:ingress` | Per-address ingress allowance | limiter |
| `{sub:<subject>}:rate` / `:quota` | Subject rate limit and quota buckets | limiter |
| `{org:<org_id>}:rate` / `:quota` | Organization rate limit and quota buckets | limiter |
| `{ip:<ip>}:org-rate` / `:org-quota` | Organization limits charged to the address | limiter |
| `{org:<org_id>}:limit-overrides` | Cached named-limit overrides of an org | 60 s |
| `{admin:<user|key>:<id>}:requests` | An admin principal's own bucket | limiter |

The named-limiter suffix is appended by `crates/lim`: `Limiter::check` turns the
key above into `<key>:<limiter name>`, after the tag, so two named limits never
share state and the key keeps its slot. The token-bucket strategy declares two
keys, `<key>:tokens` and `<key>:time`; both keep the tag. The quota tracker
keeps the window marker and the counter as fields of one hash key.

IP addresses may be IPv6 (`{ip:2001:db8::1}:ingress`): the colons belong to the
tag and do not change slot selection.

The [password login guard](config-v1.md#password-login-guard) reserves each
bucket with a single-key atomic update. The source and account keys can occupy
different slots, so a later denial refunds earlier reservations for that
request. A live `login-known` field lets that account/address pair skip the
account-wide backoff bucket. Both address-related buckets are always enforced.
Deleting the `login-known` hash forgets every recognized address of the account
in one command, with no `SCAN` or per-address index.

## Redis Cluster rules

- A command, Lua script or transaction may only touch keys with the same tag.
  The schema makes every co-accessed set share one tag by construction.
- Lua scripts declare **all** keys they touch in `KEYS`; the token bucket
  receives both of its keys, and the quota tracker derives no key inside the
  script.
- Batch store operations split their work by tag and send one pipeline per
  slot. `RedisStore::batch_get`, `batch_set`, `batch_delete` and `batch_exists`
  remain one round trip when their keys belong to one entity.
- Audit streams are the exception: `AUDIT_REDIS_STREAM` (default `audit.raw`)
  is an external contract, explicitly named and deliberately outside the
  namespaced keyspace.

## ACL

Restrict the Redis user to the schema root (and the audit stream):

```text
ACL SETUSER stargate reset on nopass ~stargate:v1:* ~audit.raw +@all -@admin -@dangerous
```

With `REDIS_KEY_PREFIX=tenant:` the pattern becomes `~tenant:stargate:v1:*`.
Redis glob patterns match the whole physical key; `{` and `}` are ordinary
characters there. Each connection needs `COMMAND INFO` to verify `HEXPIRE`
support. When replica confirmation is enabled, the ACL must also allow
`WAIT` and `EVAL`.

## Verification

`benches/scripts/test_redis_prefix.py` runs the store and limiter suites, the
gateway smoke test and the audit relay on a single instance under this ACL.
`benches/scripts/test_redis_cluster.py` runs the same store and limiter tests on
a disposable 3+3 cluster, plus raw cross-slot `MGET`/`EVAL` negative controls.
The Redis CI workflow runs both.
