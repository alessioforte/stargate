use crate::lb::{RequestContext, Strategy, Upstream};
use rand::RngExt;

pub struct Random;

impl Random {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Random {
    fn default() -> Self {
        Self::new()
    }
}

impl Strategy for Random {
    fn select<'a>(&self, upstreams: &'a [Upstream], _ctx: &RequestContext) -> Option<&'a Upstream> {
        if upstreams.is_empty() {
            return None;
        }
        let start = rand::rng().random_range(0..upstreams.len());
        super::first_available(upstreams, start)
    }

    fn name(&self) -> &'static str {
        "random"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::testutil::{ctx, down, up};

    #[test]
    fn empty_returns_none() {
        let s = Random::new();
        assert!(s.select(&[], &ctx("1.1.1.1")).is_none());
    }

    #[test]
    fn all_down_returns_none() {
        let s = Random::new();
        let ups = [down("a"), down("b")];
        assert!(s.select(&ups, &ctx("1.1.1.1")).is_none());
    }

    #[test]
    fn only_returns_available() {
        let s = Random::new();
        let ups = [down("a"), up("b"), down("c")];
        let c = ctx("1.1.1.1");
        for _ in 0..100 {
            assert_eq!(s.select(&ups, &c).unwrap().base_url, "b");
        }
    }
}
