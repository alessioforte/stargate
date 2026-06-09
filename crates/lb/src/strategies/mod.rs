pub mod ip_hash;
pub mod random;
pub mod round_robin;

use crate::lb::Upstream;

/// First available upstream scanning forward from `start` (wrapping).
///
/// Single pass with early exit: `is_available()` is evaluated at most once per
/// upstream and the scan stops at the first hit, so there is no second pass to
/// race against circuit-breaker state changes and no per-call allocation.
/// Returns `None` if the slice is empty or every upstream is unavailable.
#[inline]
pub(crate) fn first_available(upstreams: &[Upstream], start: usize) -> Option<&Upstream> {
    let n = upstreams.len();
    if n == 0 {
        return None;
    }
    let start = start % n;
    (0..n).find_map(|i| {
        let u = &upstreams[(start + i) % n];
        u.is_available().then_some(u)
    })
}

#[cfg(test)]
pub(crate) mod testutil {
    use crate::lb::{RequestContext, Upstream};

    /// Healthy upstream.
    pub fn up(url: &str) -> Upstream {
        Upstream::new(url.to_string(), None)
    }

    /// Upstream whose circuit is open (threshold 1, long cooldown) so it stays
    /// unavailable for the duration of a test.
    pub fn down(url: &str) -> Upstream {
        let u = Upstream::with_circuit_breaker(url.to_string(), None, 1, 3600);
        u.circuit_breaker.record_failure();
        u
    }

    pub fn ctx(ip: &str) -> RequestContext<'_> {
        RequestContext {
            client_ip: ip,
            path: "/",
            method: "GET",
            key: None,
        }
    }
}
