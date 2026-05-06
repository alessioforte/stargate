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
        let available_count = upstreams.iter().filter(|u| u.is_available()).count();
        if available_count == 0 {
            return None;
        }
        let mut hasher = AHasher::default();
        ctx.client_ip.hash(&mut hasher);
        let hash = hasher.finish();
        let target = (hash as usize) % available_count;
        upstreams.iter().filter(|u| u.is_available()).nth(target)
    }

    fn name(&self) -> &'static str {
        "ip_hash"
    }
}
