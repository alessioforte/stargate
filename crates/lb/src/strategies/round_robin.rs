use crate::lb::{RequestContext, Strategy, Upstream};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct RoundRobin {
    current: AtomicUsize,
}

impl RoundRobin {
    pub fn new() -> Self {
        Self {
            current: AtomicUsize::new(0),
        }
    }
}

impl Default for RoundRobin {
    fn default() -> Self {
        Self::new()
    }
}

impl Strategy for RoundRobin {
    fn select<'a>(&self, upstreams: &'a [Upstream], _ctx: &RequestContext) -> Option<&'a Upstream> {
        // Rotate over the full set; first_available probes past any down node.
        // Indexing the full slice (not just the available subset) keeps the
        // rotation stable as upstreams flap.
        let start = self.current.fetch_add(1, Ordering::Relaxed);
        super::first_available(upstreams, start)
    }

    fn name(&self) -> &'static str {
        "round_robin"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::testutil::{ctx, down, up};

    #[test]
    fn empty_returns_none() {
        let rr = RoundRobin::new();
        assert!(rr.select(&[], &ctx("1.1.1.1")).is_none());
    }

    #[test]
    fn all_down_returns_none() {
        let rr = RoundRobin::new();
        let ups = [down("a"), down("b")];
        assert!(rr.select(&ups, &ctx("1.1.1.1")).is_none());
    }

    #[test]
    fn rotates_across_available() {
        let rr = RoundRobin::new();
        let ups = [up("a"), up("b"), up("c")];
        let c = ctx("1.1.1.1");
        let seq: Vec<&str> = (0..6)
            .map(|_| rr.select(&ups, &c).unwrap().base_url.as_str())
            .collect();
        assert_eq!(seq, ["a", "b", "c", "a", "b", "c"]);
    }

    #[test]
    fn skips_unavailable() {
        let rr = RoundRobin::new();
        let ups = [up("a"), down("b"), up("c")];
        let c = ctx("1.1.1.1");
        let seq: Vec<&str> = (0..6)
            .map(|_| rr.select(&ups, &c).unwrap().base_url.as_str())
            .collect();
        assert!(!seq.contains(&"b"));
        assert_eq!(seq, ["a", "c", "c", "a", "c", "c"]);
    }
}
