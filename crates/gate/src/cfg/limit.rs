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
        durable: Option<&lim::DurableQuotas>,
    ) -> Result<Box<dyn lim::RateLimit>, super::graph::CompileError> {
        self.spec.validate().map_err(|message| {
            super::graph::CompileError::new(format!("limits.{}", self.name), message)
        })?;
        self.spec.factory(state, clock, durable).map_err(|message| {
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

    /// Whether the limit keeps a durable balance in SQL: a quota with a
    /// calendar period.
    pub(super) fn is_durable(&self) -> bool {
        matches!(&self.params, Strategy::QuotaTracker { period, .. } if !period.is_short())
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
        if let Strategy::QuotaTracker {
            limit,
            period,
            timezone,
            anchor_day,
        } = &self.params
        {
            if *limit == 0 {
                return Err("quota limit must be greater than zero");
            }
            if *limit > u64::try_from(i64::MAX).expect("i64::MAX fits in u64") {
                return Err("quota limit must not exceed i64::MAX");
            }
            if period.is_short() {
                if timezone.is_some() {
                    return Err(
                        "timezone is only valid for calendar periods (day, week, month, year)",
                    );
                }
                if anchor_day.is_some() {
                    return Err(
                        "anchor_day is only valid for calendar periods (day, week, month, year)",
                    );
                }
            } else {
                let timezone = timezone
                    .as_deref()
                    .ok_or("calendar periods require an explicit timezone")?;
                lim::strategies::quota_tracker::CalendarWindow::parse(
                    period.calendar_period(),
                    timezone,
                    *anchor_day,
                )?;
            }
        }
        Ok(())
    }

    fn factory(
        &self,
        state: Arc<lim::State>,
        clock: Arc<lim::CachedClock>,
        durable: Option<&lim::DurableQuotas>,
    ) -> Result<Box<dyn lim::RateLimit>, String> {
        self.params.factory(state, clock, durable)
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
    fn is_short(&self) -> bool {
        matches!(self, Period::Second | Period::Minute | Period::Hour)
    }

    pub fn to_short_window(&self) -> Option<lim::strategies::quota_tracker::ShortWindow> {
        match self {
            Period::Second => Some(lim::strategies::quota_tracker::ShortWindow::Second),
            Period::Minute => Some(lim::strategies::quota_tracker::ShortWindow::Minute),
            Period::Hour => Some(lim::strategies::quota_tracker::ShortWindow::Hour),
            Period::Day | Period::Week | Period::Month | Period::Year => None,
        }
    }

    pub fn calendar_period(&self) -> lim::strategies::quota_tracker::CalendarPeriod {
        use lim::strategies::quota_tracker::CalendarPeriod;
        match self {
            Period::Second | Period::Minute | Period::Hour => {
                unreachable!("short periods are not calendar periods")
            }
            Period::Day => CalendarPeriod::Day,
            Period::Week => CalendarPeriod::Week,
            Period::Month => CalendarPeriod::Month,
            Period::Year => CalendarPeriod::Year,
        }
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
        /// IANA time zone of a calendar window; required for day, week,
        /// month and year, rejected for shorter windows.
        timezone: Option<String>,
        /// Day of the month a billing window starts on (1..=28), month only.
        anchor_day: Option<u8>,
    },
}

impl Strategy {
    pub fn factory(
        &self,
        state: Arc<lim::State>,
        clock: Arc<lim::CachedClock>,
        durable: Option<&lim::DurableQuotas>,
    ) -> Result<Box<dyn lim::RateLimit>, String> {
        match self {
            Strategy::Gcra {
                max_burst,
                replenish_1_per,
            } => {
                let replenish_1_per = tools::parse_duration(replenish_1_per)
                    .map_err(|_| "invalid GCRA replenish_1_per".to_string())?
                    .to_std()
                    .map_err(|_| "invalid GCRA replenish_1_per".to_string())?;

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
            Strategy::QuotaTracker {
                limit,
                period,
                timezone,
                anchor_day,
            } => match period.to_short_window() {
                Some(window) => Ok(Box::new(lim::strategies::quota_tracker::QuotaTracker::new(
                    state, clock, *limit, window,
                ))),
                None => {
                    let durable =
                        durable.ok_or("calendar quota requires a database-backed quota ledger")?;
                    let window = lim::strategies::quota_tracker::CalendarWindow::parse(
                        period.calendar_period(),
                        timezone
                            .as_deref()
                            .ok_or("calendar periods require an explicit timezone")?,
                        *anchor_day,
                    )
                    .map_err(str::to_string)?;
                    lim::strategies::quota_tracker::DurableQuotaTracker::new(
                        durable, clock, *limit, window,
                    )
                    .map(|tracker| Box::new(tracker) as Box<dyn lim::RateLimit>)
                    .map_err(|error| error.to_string())
                }
            },
        }
    }
}

/// Whether traffic may proceed when its limiter store cannot be checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnStoreError {
    #[default]
    Allow,
    Deny,
}

impl OnStoreError {
    pub(super) fn compile(
        value: &str,
        path: impl Into<String>,
    ) -> Result<Self, super::graph::CompileError> {
        match value {
            "allow" => Ok(Self::Allow),
            "deny" => Ok(Self::Deny),
            _ => Err(super::graph::CompileError::new(
                path,
                "must be allow or deny",
            )),
        }
    }
}

pub(super) fn default_on_store_error() -> String {
    "allow".into()
}

pub(super) fn is_default_on_store_error(value: &str) -> bool {
    value == "allow"
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::*;

    struct NoopLedger;

    #[async_trait::async_trait]
    impl lim::QuotaLedger for NoopLedger {
        async fn reserve(
            &self,
            _bucket: &str,
            _definition: &str,
            _window_start: i64,
            _window_end: i64,
            _limit: u64,
            units: u64,
        ) -> lim::Result<lim::Reservation> {
            Ok(lim::Reservation::Granted(units))
        }
    }

    fn durable() -> lim::DurableQuotas {
        lim::DurableQuotas::new(Arc::new(NoopLedger))
    }

    fn state() -> Arc<lim::State> {
        Arc::new(lim::State::new())
    }

    fn limit(value: serde_json::Value) -> Limit {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn calendar_quotas_need_a_sql_ledger_and_short_windows_do_not() {
        let calendar = limit(serde_json::json!({
            "name": "monthly",
            "strategy": "quota_tracker",
            "params": {"limit": 100, "period": "month", "timezone": "Europe/Rome"}
        }));
        let clock = lim::CachedClock::new();
        let error = calendar
            .clone()
            .build(state(), clock.clone(), None)
            .err()
            .expect("calendar quota without a ledger must fail");
        assert!(error.to_string().contains("limits.monthly"));
        assert!(
            calendar
                .build(state(), clock.clone(), Some(&durable()))
                .is_ok()
        );

        let short = limit(serde_json::json!({
            "name": "hourly",
            "strategy": "quota_tracker",
            "params": {"limit": 100, "period": "hour"}
        }));
        assert!(short.build(state(), clock, None).is_ok());
    }

    #[test]
    fn durable_limits_report_their_failure_mode() {
        let calendar = limit(serde_json::json!({
            "name": "monthly",
            "strategy": "quota_tracker",
            "params": {"limit": 100, "period": "month", "timezone": "UTC"}
        }));
        let clock = lim::CachedClock::new();
        let durable = calendar
            .build(state(), clock.clone(), Some(&durable()))
            .unwrap();
        assert!(durable.is_durable());

        let short = limit(serde_json::json!({
            "name": "hourly",
            "strategy": "quota_tracker",
            "params": {"limit": 100, "period": "hour"}
        }));
        assert!(!short.build(state(), clock, None).unwrap().is_durable());
    }
}
