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
    pub fn build(
        self,
        state: Arc<lim::State>,
        clock: Arc<lim::CachedClock>,
    ) -> Box<dyn lim::RateLimit> {
        self.params.factory(state, clock)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
enum Period {
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

impl Period {
    pub fn to_time_window(&self) -> lim::strategies::quota_tracker::TimeWindow {
        match self {
            Period::Second => lim::strategies::quota_tracker::TimeWindow::Second,
            Period::Minute => lim::strategies::quota_tracker::TimeWindow::Minute,
            Period::Hour => lim::strategies::quota_tracker::TimeWindow::Hour,
            Period::Day => lim::strategies::quota_tracker::TimeWindow::Day,
            Period::Week => lim::strategies::quota_tracker::TimeWindow::Week,
            Period::Month => lim::strategies::quota_tracker::TimeWindow::Month,
            Period::Year => lim::strategies::quota_tracker::TimeWindow::Year,
        }
    }
}

impl From<Period> for lim::strategies::quota_tracker::TimeWindow {
    fn from(period: Period) -> Self {
        period.to_time_window()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "strategy", content = "params", rename_all = "snake_case")]
enum Strategy {
    Gcra {
        max_burst: u32,
        replenish_1_per: String,
    },
    TokenBucket {
        capacity: u64,
        refill_rate: u64,
    },
    QuotaTracker {
        limit: u64,
        period: Period,
    },
}

impl Strategy {
    pub fn factory(
        &self,
        state: Arc<lim::State>,
        clock: Arc<lim::CachedClock>,
    ) -> Box<dyn lim::RateLimit> {
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
                capacity,
                refill_rate,
            } => {
                let config = lim::token_bucket::TokenBucketConfig::new(*capacity, *refill_rate);
                Box::new(lim::token_bucket::TokenBucket::new(state, clock, config))
            }
            Strategy::QuotaTracker { limit, period } => {
                let time_window = period.to_time_window();
                Box::new(lim::strategies::quota_tracker::QuotaTracker::new(
                    state,
                    clock,
                    *limit,
                    time_window,
                ))
            }
        }
    }
}
