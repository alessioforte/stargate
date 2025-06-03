use super::{LoadBalancer, RequestContext, Upstream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub struct RoundRobin {
    current: AtomicUsize,
    upstreams: Vec<Upstream>,
}

impl RoundRobin {
    pub fn new(upstreams: Vec<Upstream>) -> Arc<Self> {
        Arc::new(Self {
            current: AtomicUsize::new(0),
            upstreams,
        })
    }
}

impl LoadBalancer for RoundRobin {
    fn select(&self, _ctx: &RequestContext) -> Option<&Upstream> {
        if self.upstreams.is_empty() {
            return None;
        }
        let index = self.current.fetch_add(1, Ordering::SeqCst) % self.upstreams.len();
        Some(&self.upstreams[index])
    }

    fn name(&self) -> &'static str {
        "round_robin"
    }
}
