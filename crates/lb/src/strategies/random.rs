use crate::lb::{RequestContext, Strategy, Upstream};
use rand::Rng;

pub struct Random;

impl Random {
    pub fn new() -> Self {
        Self
    }
}

impl Strategy for Random {
    fn select<'a>(&self, alive: &Vec<&'a Upstream>, _ctx: &RequestContext) -> Option<&'a Upstream> {
        if alive.is_empty() {
            return None;
        }
        let mut rng = rand::rng();
        let index = rng.random_range(0..alive.len());
        Some(&alive[index])
    }

    fn name(&self) -> &'static str {
        "random"
    }
}
