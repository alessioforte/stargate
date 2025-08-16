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

impl Strategy for RoundRobin {
    fn select<'a>(&self, alive: &Vec<&'a Upstream>, _ctx: &RequestContext) -> Option<&'a Upstream> {
        if alive.is_empty() {
            return None;
        }
        let index = self.current.fetch_add(1, Ordering::SeqCst) % alive.len();
        Some(&alive[index])
    }

    fn name(&self) -> &'static str {
        "round_robin"
    }
}
