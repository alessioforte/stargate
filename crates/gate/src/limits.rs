#[derive(sqlx::Type, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[sqlx(type_name = "reset_period", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ResetPeriod {
    Daily,
    Weekly,
    Monthly,
    Yearly,
    Never, // For lifetime quotas
}

impl ToString for ResetPeriod {
    fn to_string(&self) -> String {
        match self {
            ResetPeriod::Daily => "daily".to_string(),
            ResetPeriod::Weekly => "weekly".to_string(),
            ResetPeriod::Monthly => "monthly".to_string(),
            ResetPeriod::Yearly => "yearly".to_string(),
            ResetPeriod::Never => "never".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Limits {
    /// Maximum number of requests allowed per second.
    pub requests_per_second: u32,

    /// Maximum burst size - number of requests that can be made in a short burst
    pub burst_size: u32,

    /// Time window in seconds for rate limiting.
    pub window_size_seconds: u64,

    /// Quotas for different reset periods.
    pub quotas: Option<std::collections::HashMap<ResetPeriod, u64>>,
}

impl Limits {
    pub fn new(
        requests_per_second: u32,
        burst_size: u32,
        window_size_seconds: u64,
        quotas: Option<std::collections::HashMap<ResetPeriod, u64>>,
    ) -> Self {
        Limits {
            requests_per_second,
            burst_size,
            window_size_seconds,
            quotas,
        }
    }
}
