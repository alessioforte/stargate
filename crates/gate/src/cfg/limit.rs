use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Limit {
    name: String,
    #[serde(flatten)]
    params: Strategy,
}

impl Default for Limit {
    fn default() -> Self {
        Limit {
            name: "default".to_string(),
            params: Strategy::Gcra {
                max_burst: 2,
                replenish_1_per: "5s".to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "strategy", content = "params", rename_all = "snake_case")]
enum Strategy {
    // FixedWindow { window: u64, limit: u64 },
    // SlidingLog { window: u64, limit: u64 },
    // SlidingWindow { window: u64, limit: u64 },
    TokenBucket {
        refill_rate: u64,
        capacity: u64,
    },
    Gcra {
        max_burst: u64,
        replenish_1_per: String,
    },
}

impl Strategy {
    // pub fn factory(&self) -> Box<dyn RateLimit> {
    pub fn factory(&self) {
        match self {
            Strategy::Gcra {
                max_burst,
                replenish_1_per,
            } => {
                // Return a GCRA rate limiter instance
                // Box::new(Gcra::new(...))
                unimplemented!()
            }
            Strategy::TokenBucket {
                refill_rate,
                capacity,
            } => {
                // Return a Token Bucket rate limiter instance
                // Box::new(TokenBucket::new(...))
                unimplemented!()
            }
        }
    }
}
