use core::num::NonZeroU32;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Limit {
    pub name: String,
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

impl Limit {
    pub fn build(self, state: Arc<lim::State>) -> Box<dyn lim::RateLimit> {
        self.params.factory(state)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "strategy", content = "params", rename_all = "snake_case")]
enum Strategy {
    TokenBucket {
        refill_rate: u64,
        capacity: u64,
    },
    Gcra {
        max_burst: u32,
        replenish_1_per: String,
    },
}

impl Strategy {
    pub fn factory(&self, state: Arc<lim::State>) -> Box<dyn lim::RateLimit> {
        match self {
            Strategy::Gcra {
                max_burst,
                replenish_1_per,
            } => {
                let replenish_1_per = match tools::parse_duration(replenish_1_per) {
                    Ok(dur) => dur.to_std().unwrap(),
                    Err(_) => chrono::Duration::seconds(5).to_std().unwrap(),
                };

                let quota = lim::gcra::Quota {
                    max_burst: NonZeroU32::new(*max_burst).unwrap(),
                    replenish_1_per,
                };
                Box::new(lim::gcra::Gcra::new(state, quota))
            }
            Strategy::TokenBucket {
                refill_rate,
                capacity,
            } => {
                unimplemented!()
            }
        }
    }
}
