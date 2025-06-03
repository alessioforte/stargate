use super::{LoadBalancer, RequestContext, Upstream};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

pub struct IpHash {
    upstreams: Vec<Upstream>,
}

impl IpHash {
    pub fn new(upstreams: Vec<Upstream>) -> Arc<Self> {
        Arc::new(Self { upstreams })
    }
}

impl LoadBalancer for IpHash {
    fn select(&self, ctx: &RequestContext) -> Option<&Upstream> {
        if self.upstreams.is_empty() || ctx.client_ip.is_none() {
            return None;
        }
        let ip = ctx.client_ip.unwrap();
        let mut hasher = DefaultHasher::new();
        ip.hash(&mut hasher);
        let hash = hasher.finish();
        let index = (hash as usize) % self.upstreams.len();
        Some(&self.upstreams[index])
    }

    fn name(&self) -> &'static str {
        "ip_hash"
    }
}
