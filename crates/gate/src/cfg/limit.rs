use core::num::NonZeroU32;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Limit {
    pub name: String,
    #[serde(flatten)]
    spec: LimitSpec,
}

impl Default for Limit {
    fn default() -> Self {
        Limit {
            name: "default".to_string(),
            spec: LimitSpec::default(),
        }
    }
}

impl Limit {
    pub fn new(name: impl Into<String>, spec: LimitSpec) -> Self {
        Self {
            name: name.into(),
            spec,
        }
    }

    pub fn build(
        self,
        state: Arc<lim::State>,
        clock: Arc<lim::CachedClock>,
    ) -> Result<Box<dyn lim::RateLimit>, super::graph::CompileError> {
        self.spec.validate().map_err(|message| {
            super::graph::CompileError::new(format!("limits.{}", self.name), message)
        })?;
        self.spec.factory(state, clock).map_err(|message| {
            super::graph::CompileError::new(format!("limits.{}", self.name), message)
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct LimitSpec {
    #[serde(flatten)]
    params: Strategy,
}

impl Default for LimitSpec {
    fn default() -> Self {
        Self {
            params: Strategy::Gcra {
                max_burst: 2,
                replenish_1_per: "5s".to_string(),
            },
        }
    }
}

impl LimitSpec {
    pub fn kind(&self) -> lim::LimitKind {
        match self.params {
            Strategy::Gcra { .. } | Strategy::TokenBucket { .. } => lim::LimitKind::Rate,
            Strategy::QuotaTracker { .. } => lim::LimitKind::Quota,
        }
    }

    pub(super) fn resource_default() -> Self {
        Self {
            params: Strategy::Gcra {
                max_burst: 100,
                replenish_1_per: "1s".into(),
            },
        }
    }

    pub(super) fn ingress_default() -> Self {
        Self {
            params: Strategy::Gcra {
                max_burst: 100,
                replenish_1_per: "100ms".into(),
            },
        }
    }

    pub(super) fn validate_rate(&self) -> Result<(), &'static str> {
        match &self.params {
            Strategy::Gcra { .. } => self.validate(),
            Strategy::TokenBucket {
                capacity,
                refill_rate,
            } => {
                if *capacity == 0 || *refill_rate == 0 {
                    return Err("token-bucket capacity and refill_rate must be greater than zero");
                }
                // Both backends retain bucket state for refill time + 60s.
                // Reject an overflowing lifetime before constructing a bucket.
                capacity
                    .checked_div(*refill_rate)
                    .and_then(|seconds| seconds.checked_add(60))
                    .filter(|seconds| {
                        std::time::Instant::now()
                            .checked_add(std::time::Duration::from_secs(*seconds))
                            .is_some()
                    })
                    .ok_or("token-bucket lifetime exceeds the supported range")?;
                Ok(())
            }
            Strategy::QuotaTracker { .. } => {
                Err("must reference a gcra or token_bucket rate limit")
            }
        }
    }

    pub(super) fn validate(&self) -> Result<(), &'static str> {
        if let Strategy::Gcra {
            max_burst,
            replenish_1_per,
        } = &self.params
        {
            if *max_burst == 0 {
                return Err("GCRA max_burst must be greater than zero");
            }
            let duration = tools::parse_duration(replenish_1_per)
                .ok()
                .and_then(|duration| duration.to_std().ok())
                .filter(|duration| !duration.is_zero())
                .ok_or("GCRA replenish_1_per must be a positive, representable duration")?;
            let tau = duration.as_micros().max(1);
            if tau
                .checked_mul(u128::from(*max_burst))
                .is_none_or(|burst| burst > i64::MAX as u128)
            {
                return Err("GCRA duration and burst exceed the supported timestamp range");
            }
        }
        Ok(())
    }

    fn factory(
        &self,
        state: Arc<lim::State>,
        clock: Arc<lim::CachedClock>,
    ) -> Result<Box<dyn lim::RateLimit>, &'static str> {
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
    ) -> Result<Box<dyn lim::RateLimit>, &'static str> {
        match self {
            Strategy::Gcra {
                max_burst,
                replenish_1_per,
            } => {
                let replenish_1_per = tools::parse_duration(replenish_1_per)
                    .map_err(|_| "invalid GCRA replenish_1_per")?
                    .to_std()
                    .map_err(|_| "invalid GCRA replenish_1_per")?;

                let quota = lim::gcra::Quota {
                    max_burst: NonZeroU32::new(*max_burst)
                        .ok_or("GCRA max_burst must be greater than zero")?,
                    replenish_1_per,
                };
                Ok(Box::new(lim::gcra::Gcra::new(state, clock, quota)))
            }
            Strategy::TokenBucket {
                capacity,
                refill_rate,
            } => {
                let config = lim::token_bucket::TokenBucketConfig::new(*capacity, *refill_rate);
                Ok(Box::new(lim::token_bucket::TokenBucket::new(
                    state, clock, config,
                )))
            }
            Strategy::QuotaTracker { limit, period } => {
                let time_window = period.to_time_window();
                Ok(Box::new(lim::strategies::quota_tracker::QuotaTracker::new(
                    state,
                    clock,
                    *limit,
                    time_window,
                )))
            }
        }
    }
}
