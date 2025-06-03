use super::{LoadBalancer, RequestContext, Upstream};
use rand::Rng;
use std::sync::Arc;

pub struct Random {
    upstreams: Vec<Upstream>,
}

impl Random {
    pub fn new(upstreams: Vec<Upstream>) -> Arc<Self> {
        Arc::new(Self { upstreams })
    }
}

impl LoadBalancer for Random {
    fn select(&self, _ctx: &RequestContext) -> Option<&Upstream> {
        if self.upstreams.is_empty() {
            return None;
        }
        let mut rng = rand::rng();
        let index = rng.random_range(0..self.upstreams.len());
        Some(&self.upstreams[index])
    }

    fn name(&self) -> &'static str {
        "random"
    }
}
