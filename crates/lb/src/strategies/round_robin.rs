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
        let available_count = upstreams.iter().filter(|u| u.is_available()).count();
        if available_count == 0 {
            return None;
        }
        let target = self.current.fetch_add(1, Ordering::Relaxed) % available_count;
        upstreams.iter().filter(|u| u.is_available()).nth(target)
    }

    fn name(&self) -> &'static str {
        "round_robin"
    }
}
