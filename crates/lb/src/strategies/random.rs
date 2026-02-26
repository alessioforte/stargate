use crate::lb::{RequestContext, Strategy, Upstream};
use rand::Rng;

pub struct Random;

impl Random {
    pub fn new() -> Self {
        Self
    }
}

impl Strategy for Random {
    fn select<'a>(&self, upstreams: &'a [Upstream], _ctx: &RequestContext) -> Option<&'a Upstream> {
        let available_count = upstreams.iter().filter(|u| u.is_available()).count();
        if available_count == 0 {
            return None;
        }
        let mut rng = rand::rng();
        let target = rng.random_range(0..available_count);
        upstreams.iter().filter(|u| u.is_available()).nth(target)
    }

    fn name(&self) -> &'static str {
        "random"
    }
}
