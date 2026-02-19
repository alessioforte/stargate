use crate::lb::{RequestContext, Strategy, Upstream};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub struct IpHash;

impl IpHash {
    pub fn new() -> Self {
        Self
    }
}

impl Strategy for IpHash {
    fn select<'a>(&self, alive: &Vec<&'a Upstream>, ctx: &RequestContext) -> Option<&'a Upstream> {
        if alive.is_empty() {
            return None;
        }
        let ip = ctx.client_ip.clone();
        let mut hasher = DefaultHasher::new();
        ip.hash(&mut hasher);
        let hash = hasher.finish();
        let index = (hash as usize) % alive.len();
        Some(&alive[index])
    }

    fn name(&self) -> &'static str {
        "ip_hash"
    }
}
