# Lim — Rate Limiting & Quota for Stargate

Async rate-limiting and quota crate. Pluggable strategies (GCRA, token bucket, fixed-window quota tracker), with a single `Limiter` registry that maps a name to a strategy and exposes one `check(name, key, cost)` API. Backend is feature-selected at compile time: in-memory (`memory`) for single-node, Redis (`redis`) for clustered deployments.

## Features

- **Three strategies** — `Gcra`, `TokenBucket`, `QuotaTracker` (fixed-window counter).
- **Compile-time backend** — `memory` (DashMap) or `redis` (atomic Lua scripts), exactly one per build.
- **Identical semantics across backends** — same return values, same units (millisecond-resolution `retry_after`).
- **Named registry** — `Limiter` holds many limits, dispatch by string name.
- **Cost-aware** — every `check` accepts a `cost` (defaults to 1 via `Limiter::check`).
- **Cached clock** — `CachedClock` updates every 10ms in a background thread; avoids per-call `SystemTime::now()`.
- **Quota windows** — second / minute / hour / day / week / month / year / custom.
- **Rich decision** — limit, remaining, retry_after, reset, plus helpers for HTTP `Retry-After` headers.
- **Strict named references** — `Limiter::check` returns `RateLimitError::UnknownLimit` for an unregistered name. Skipping a check is an explicit caller decision.
- **Typed strategies** — every strategy exposes `LimitKind::Rate` or `LimitKind::Quota`; `Limiter::validate_limit` rejects unknown or incompatible references without consuming state.

## Backend Selection

Exactly one backend feature must be enabled. The crate emits `compile_error!` otherwise.

```toml
[dependencies]
lim = { path = "../lim", features = ["memory"] }   # single-node
# or
lim = { path = "../lim", features = ["redis"]  }   # cluster
```

## Strategies

### GCRA — `strategies::gcra::Gcra`

Generic Cell Rate Algorithm. Smooth, single-key state (one TAT per key). Ideal for steady-rate enforcement with bounded burst.

Configured via `Quota`:

| Field             | Meaning                                  |
|-------------------|------------------------------------------|
| `max_burst`       | Max tokens consumable in one burst (`NonZeroU32`) |
| `replenish_1_per` | Time to replenish one token (`Duration`) |

Constructors:

```rust
use lim::strategies::gcra::Quota;

Quota::per_second(100);                 // 100 req/s, burst 100
Quota::per_minute(60);                  // 60 req/min, burst 60
Quota::per_hour(3600);                  // 3600 req/h, burst 3600
Quota::per_duration(100, dur);          // arbitrary window
Quota::per_second(10).with_burst(50);   // 10 req/s sustained, burst 50
```

Internally: `tau = replenish_1_per` (µs), `burst = tau * (max_burst - 1)`, TTL ≥ 60s.

### Token Bucket — `strategies::token_bucket::TokenBucket`

Capacity + refill rate. Cost per request supported (good for weighted operations).

Configured via `TokenBucketConfig`:

| Field         | Meaning                                |
|---------------|----------------------------------------|
| `capacity`    | Max tokens (burst)                     |
| `refill_rate` | Tokens added per second                |

```rust
use lim::strategies::token_bucket::TokenBucketConfig;

TokenBucketConfig::new(100, 10);                 // cap 100, refill 10/s
TokenBucketConfig::per_second(50);               // cap 50, refill 50/s
TokenBucketConfig::per_minute(120);              // cap 120, refill 2/s
TokenBucketConfig::per_hour(3600);               // cap 3600, refill 1/s
TokenBucketConfig::per_second(10).with_burst(100);
TokenBucketConfig::new(100, 10).with_refill_rate(5);
```

Memory impl scales tokens by 1000 internally to preserve fractional refill across denied checks. `retry_after` is computed from the deficit and refill rate (millisecond ceiling). `refill_rate == 0` returns a sentinel `retry_after` of `u32::MAX` ms.

### Quota Tracker — `strategies::quota_tracker::QuotaTracker`

Fixed-window counter. Aligned to wall-clock boundaries (e.g. minute starts at second 0).

```rust
use lim::strategies::quota_tracker::{QuotaTracker, TimeWindow};

QuotaTracker::new(store, clock, 1_000, TimeWindow::Hour);   // 1k req/hour
```

`TimeWindow`: `Second`, `Minute`, `Hour`, `Day`, `Week`, `Month` (avg), `Year` (avg), `Custom(Duration)`. Parses from `&str` (`"minute"`, `"hours"`, etc.); unknown strings fall back to `Custom(300s)`.

Key form: `{key}:{window_secs}:{window_start}` — windows rotate naturally as the timestamp advances. TTL = window + 1s.

## Limiter Registry

```rust
use lim::Limiter;

let mut limiter = Limiter::new();
limiter.add_limit("login".into(), Box::new(gcra));
limiter.add_limit("api".into(),   Box::new(token_bucket));

let decision = limiter.check("login", "user:42", None).await?;
if !decision.is_allowed() {
    let retry = decision.retry_after_secs();
    // emit 429
}
```

`Limiter::check` validates that `key` is non-empty and ≤ 256 chars. If `limiter_name` is unknown, it returns an `allowed` decision — fail-open. Override the cost via the `cost: Option<u64>` parameter (defaults to 1).

## Decision

`RateLimitDecision`:

| Field         | Type                | Notes                                       |
|---------------|---------------------|---------------------------------------------|
| `allowed`     | `bool`              | Outcome                                     |
| `limit`       | `u64`               | Configured capacity / rate                  |
| `remaining`   | `u64`               | Remaining capacity after this check         |
| `retry_after` | `Option<Duration>`  | When denied; millisecond resolution         |
| `reset`       | `Option<Duration>`  | When current window/state resets            |

Helpers: `is_allowed`, `is_rate_limited`, `is_exhausted`, `retry_after_secs` (ceiling, for HTTP `Retry-After`), `retry_after_millis`.

## Clock

`CachedClock::new()` returns `Arc<CachedClock>` and spawns a background thread that refreshes a `SystemTime`-derived microsecond timestamp every 10ms. Strategies read it lock-free via `Ordering::Relaxed`. Trade ~10ms timestamp granularity for zero syscall on the hot path.

```rust
use lim::CachedClock;

let clock = CachedClock::new();
clock.now_micros();
clock.now_millis();
clock.now_secs();
```

## State

`State` is a type alias re-exported by feature:

- `feature = "memory"` → `store::MemoryStore` (DashMap, atomic CAS via `measure_and_set` / `measure_and_set_i64` / `incr_i64`).
- `feature = "redis"`  → `store::RedisStore` (Lua scripts loaded once via `LazyLock`, single-round-trip atomic execution).

All strategy state is keyed strings + per-key TTL. No cross-key locks.

## Quick Start

```rust
use std::sync::Arc;
use lim::{CachedClock, Limiter, State};
use lim::strategies::gcra::{Gcra, Quota};

let store = Arc::new(State::new(/* backend args */));
let clock = CachedClock::new();

let gcra = Gcra::new(store.clone(), clock.clone(), Quota::per_second(100));

let mut limiter = Limiter::new();
limiter.add_limit("api".into(), Box::new(gcra));

let decision = limiter.check("api", "tenant:acme", Some(1)).await?;
if decision.is_allowed() {
    // forward request
} else {
    // return 429 with Retry-After: decision.retry_after_secs()
}
```

## Errors

`RateLimitError`:

- `RateLimitExceeded`
- `InvalidConfig(String)` — bad key, malformed quota
- `BackendError(String)` — Redis script failure
- `MemoryError(String)` — in-memory store failure
- `ClockError(String)`
- `RedisError(redis::RedisError)` — `#[from]`, `feature = "redis"` only

## Notes

- Choose **one** strategy per limit-name; dispatch is by name in `Limiter`.
- GCRA ignores `cost`; use Token Bucket if you need weighted requests.
- Quota Tracker is window-aligned (boundary effects at window edges); use GCRA or Token Bucket for smoother enforcement.
- `Quota::per_second(0)` and `with_burst(0)` clamp to 1 (avoids divide-by-zero downstream).
- Redis impls use `LazyLock<RedisScript>` — Lua source lives next to the Rust file (`gcra.lua`, `token_bucket.lua`, `quota_tracker.lua`).
