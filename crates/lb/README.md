# lb

Load balancing, active health checking, and circuit breaking for the Stargate gateway. Provides a pluggable `Strategy` trait, three built-in strategies (`round_robin`, `random`, `ip_hash`), a lock-free per-upstream `CircuitBreaker`, and a `HealthCheck` registry that runs periodic liveness probes.

Backend-agnostic — no Cargo feature flags. Used in both the edge and cluster deployment profiles.

## Architecture

```
crates/lb/
├── src/
│   ├── lib.rs                 # Public re-exports
│   ├── lb.rs                  # LoadBalancer + Strategy traits, Upstream, RequestContext, BaseLoadBalancer
│   ├── circuit_breaker.rs     # Lock-free CircuitBreaker state machine
│   ├── health_check.rs        # HealthCheck registry + active probe tasks
│   └── strategies/
│       ├── mod.rs             # first_available() probe helper + strategy modules
│       ├── round_robin.rs     # RoundRobin
│       ├── random.rs          # Random
│       └── ip_hash.rs         # IpHash
└── benches/
    └── select.rs              # Criterion bench for select()
```

---

## Core traits

### LoadBalancer

The object the gateway holds (`Arc<dyn LoadBalancer + Send + Sync>`). One instance fronts one upstream pool.

```rust
#[async_trait]
pub trait LoadBalancer {
    fn select(&self, context: &RequestContext) -> Option<&Upstream>;
    fn name(&self) -> &'static str;
    fn mark_alive(&self, base_url: &str);   // record a healthy result for an upstream
    fn mark_dead(&self, base_url: &str);    // record a failed result for an upstream
    async fn health_check(&self);           // run one active probe round over all upstreams
}
```

`select` returns `None` when every upstream is unavailable (circuit open). `mark_alive` / `mark_dead` feed the per-upstream circuit breaker by `base_url`.

### Strategy

The pluggable selection policy. `BaseLoadBalancer<S>` adapts any `Strategy` into a `LoadBalancer`.

```rust
pub trait Strategy: Send + Sync {
    fn select<'a>(&self, upstreams: &'a [Upstream], context: &RequestContext) -> Option<&'a Upstream>;
    fn name(&self) -> &'static str;
}
```

### RequestContext

Read-only request facts passed to a strategy. Only `client_ip` is consumed today (by `ip_hash`); the rest are available for future header/path/identity-aware strategies.

```rust
pub struct RequestContext<'a> {
    pub client_ip: &'a str,
    pub path: &'a str,
    pub method: &'a str,
    pub key: Option<&'a str>,   // authenticated subject id, when present
}
```

---

## Upstream

A single backend target plus its circuit breaker.

```rust
let upstream = Upstream::new("http://api-a:8080".into(), Some("/health".into()));

// Explicit breaker tuning (threshold, cooldown seconds):
let upstream = Upstream::with_circuit_breaker(
    "http://api-a:8080".into(),
    Some("/health".into()),
    3,   // fail_threshold
    30,  // cooldown_secs
);

upstream.is_available();       // breaker allows traffic?
upstream.health_check_url();   // Option<String> = base_url + health_check_path
```

`Upstream::new` defaults to `fail_threshold = 3`, `cooldown_secs = 30`.

---

## Strategies

| Strategy      | `name()`        | Selection                                                                 |
|---------------|-----------------|---------------------------------------------------------------------------|
| `RoundRobin`  | `round_robin`   | Atomic counter, modulo the full pool, probe forward past unavailable nodes |
| `Random`      | `random`        | Random start index, probe forward                                          |
| `IpHash`      | `ip_hash`       | `ahash(client_ip)` start index, probe forward                              |

### Selection algorithm

All strategies share `first_available(upstreams, start)`: compute a start index into the **full** pool, then linear-probe forward (wrapping) for the first available upstream.

- **Single pass, early exit.** `is_available()` is evaluated at most once per upstream and the scan stops at the first hit.
- **Zero allocation.** No intermediate `Vec` of candidates — selection runs entirely on the borrowed slice.
- **Stable mapping.** Indexing the full pool (not the available subset) keeps `round_robin` rotation and `ip_hash` assignment stable as upstreams flap; a client only remaps when its own target is down.

This avoids the classic two-scan race where availability changes between counting and indexing (`is_available()` can mutate breaker state from `Open` to `HalfOpen`).

---

## BaseLoadBalancer

Wraps a `Strategy` + a `Vec<Upstream>` and builds the shared HTTP client used for active probes.

```rust
use lb::{BaseLoadBalancer, RoundRobin, Upstream};

let upstreams = vec![
    Upstream::new("http://api-a:8080".into(), Some("/health".into())),
    Upstream::new("http://api-b:8080".into(), Some("/health".into())),
];
let lb = BaseLoadBalancer::new(RoundRobin::new(), upstreams); // -> Arc<BaseLoadBalancer<RoundRobin>>
```

`mark_alive` / `mark_dead` resolve an upstream via an ahash-keyed `base_url → index` map. The probe client uses a 2s connect timeout and 2s request timeout.

---

## CircuitBreaker

Per-upstream, lock-free, three-state breaker built entirely on atomics (`AtomicU8` state, `AtomicUsize` fail count, `AtomicU64` opened-at timestamp). No mutex on the request path.

```
      success            N consecutive failures
┌──── Closed ────────────────────► Open ──────┐
│       ▲                           │          │
│       │ success      cooldown elapsed        │ failure
│       │                           │          │
│       └─────── HalfOpen ◄─────────┘          │
│                    │                         │
│                    └── failure ──► Open ◄────┘
└──────────────────────────────────────────────┘
```

| State (`state()`) | Value | Behaviour                                                      |
|-------------------|-------|---------------------------------------------------------------|
| `Closed`          | `0`   | All traffic allowed.                                          |
| `Open`            | `1`   | Traffic rejected until `cooldown` elapses.                   |
| `HalfOpen`        | `2`   | A single probe request is admitted to test recovery.         |

```rust
let cb = CircuitBreaker::new(3, 30); // open after 3 consecutive failures, 30s cooldown
cb.is_available();    // also drives Open -> HalfOpen transition once cooldown elapses
cb.record_success();  // -> Closed, resets fail count
cb.record_failure();  // increments; opens at threshold (or immediately if a HalfOpen probe fails)
```

Design notes:

- **Single-probe half-open gate.** Exactly one caller wins the `Open → HalfOpen` transition (CAS) and is admitted; concurrent callers are rejected. This prevents a recovery stampede onto a still-fragile upstream.
- **Stall re-arm.** If an admitted probe never reports back, the gate re-arms after another cooldown window so a lost probe cannot wedge the circuit open forever.
- **Read-only success fast path.** When already `Closed` with zero failures, `record_success` returns without writing — so the per-request `mark_alive` feedback stays read-only in steady state and avoids cache-line contention across cores.

---

## HealthCheck

Registry of load balancers grouped by probe interval. Each interval gets one Tokio task that probes its balancers in a loop.

```rust
use lb::HealthCheck;
use chrono::Duration;

let mut hc = HealthCheck::new();
hc.register(Duration::seconds(5), Arc::clone(&lb));  // group by interval
hc.run();   // spawn one task per interval
// ...
hc.stop();  // abort all probe tasks
```

`health_check()` probes every upstream that has a `health_check_url()`: a `2xx` response calls `record_success`, anything else (non-success status or transport error) calls `record_failure`.

---

## Gateway integration

The breaker is driven by **two independent signals**:

1. **Active liveness probes** — `HealthCheck` polls `health_check_path` on each upstream.
2. **Passive live traffic** — the gateway executor calls `mark_alive` / `mark_dead` based on real proxied request outcomes:
   - transport errors and `502`/`503`/`504` responses → `mark_dead`
   - any other received response → `mark_alive`
   - mirror (shadow) traffic is excluded, so it cannot eject real upstreams

Breaker threshold and cooldown are exposed per upstream in the v2 gateway config (`crates/gate`):

```yaml
http:
  upstreams:
    api:
      targets:
        - url: http://api-a:8080
      load_balancer:
        strategy: round_robin
        liveness_probe:
          path: /health
          interval: 5s
        circuit_breaker:
          fail_threshold: 3   # default 3
          cooldown: 30s       # default 30s
```

Breaker state is per-process; it is not shared across cluster nodes.

---

## Quick start

```rust
use lb::{BaseLoadBalancer, LoadBalancer, RequestContext, RoundRobin, Upstream};

let lb = BaseLoadBalancer::new(
    RoundRobin::new(),
    vec![
        Upstream::new("http://api-a:8080".into(), Some("/health".into())),
        Upstream::new("http://api-b:8080".into(), Some("/health".into())),
    ],
);

let ctx = RequestContext { client_ip: "203.0.113.7", path: "/v1/things", method: "GET", key: None };

match lb.select(&ctx) {
    Some(upstream) => {
        // ... proxy the request to upstream.base_url, then feed the result back:
        lb.mark_alive(&upstream.base_url); // or mark_dead on transport error / 502/503/504
    }
    None => { /* all upstreams unavailable -> 503 */ }
}
```

---

## Benchmarks

`select()` is on the request hot path. A Criterion bench covers all three strategies, healthy and half-down pools:

```bash
cargo bench -p lb
```

Indicative results (8 upstreams):

| scenario           | round_robin | random  | ip_hash |
|--------------------|-------------|---------|---------|
| all healthy        | ~28 ns      | ~28 ns  | ~25 ns  |
| half down (probe)  | ~59 ns      | ~58 ns  | ~97 ns  |

Selection is allocation-free in all cases.
