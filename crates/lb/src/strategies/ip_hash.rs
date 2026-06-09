use crate::lb::{RequestContext, Strategy, Upstream};
use ahash::AHasher;
use std::hash::{Hash, Hasher};

pub struct IpHash;

impl IpHash {
    pub fn new() -> Self {
        Self
    }
}

impl Default for IpHash {
    fn default() -> Self {
        Self::new()
    }
}

impl Strategy for IpHash {
    fn select<'a>(&self, upstreams: &'a [Upstream], ctx: &RequestContext) -> Option<&'a Upstream> {
        if upstreams.is_empty() {
            return None;
        }
        // Hash into the full set so a client maps to a stable upstream; only
        // remap (probe forward) when that upstream is unavailable.
        let mut hasher = AHasher::default();
        ctx.client_ip.hash(&mut hasher);
        let start = (hasher.finish() % upstreams.len() as u64) as usize;
        super::first_available(upstreams, start)
    }

    fn name(&self) -> &'static str {
        "ip_hash"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::testutil::{ctx, down, up};

    #[test]
    fn empty_returns_none() {
        let s = IpHash::new();
        assert!(s.select(&[], &ctx("1.2.3.4")).is_none());
    }

    #[test]
    fn same_ip_is_stable() {
        let s = IpHash::new();
        let ups = [up("a"), up("b"), up("c")];
        let c = ctx("203.0.113.7");
        let first = s.select(&ups, &c).unwrap().base_url.clone();
        for _ in 0..50 {
            assert_eq!(s.select(&ups, &c).unwrap().base_url, first);
        }
    }

    #[test]
    fn distributes_across_ips() {
        let s = IpHash::new();
        let ups = [up("a"), up("b"), up("c")];
        let mut seen = std::collections::HashSet::new();
        for i in 0..60u8 {
            let ip = format!("10.0.0.{i}");
            let c = ctx(&ip);
            seen.insert(s.select(&ups, &c).unwrap().base_url.clone());
        }
        assert!(seen.len() > 1, "ip_hash should spread across upstreams");
    }

    #[test]
    fn probes_past_unavailable() {
        let s = IpHash::new();
        // only "b" is up → every client must land on it regardless of hash.
        let ups = [down("a"), up("b"), down("c")];
        for i in 0..30u8 {
            let ip = format!("10.0.0.{i}");
            let c = ctx(&ip);
            assert_eq!(s.select(&ups, &c).unwrap().base_url, "b");
        }
    }
}
