use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject {
    pub id: String,
    #[serde(rename = "type")]
    pub sub_type: String,
    pub sub_id: String,
    pub attrs: serde_json::Value,
    pub limits: Option<rl::RateLimitConfig>,
}

impl From<db::ent::Subject> for Subject {
    fn from(subject: db::ent::Subject) -> Self {
        let limits = match subject.limits {
            Some(l) => {
                let quotas = match l.quotas.clone() {
                    Some(q) => {
                        let mut map = std::collections::HashMap::new();
                        for (k, v) in q {
                            if let Ok(rp) = rl::config::ResetPeriod::from_str(&k.to_string()) {
                                map.insert(rp, v);
                            }
                        }
                        Some(map)
                    }
                    None => None,
                };
                Some(rl::RateLimitConfig {
                    requests_per_second: l.requests_per_second,
                    burst_size: l.burst_size,
                    window_size_seconds: l.window_size_seconds,
                    quotas,
                })
            }
            None => None,
        };
        Self {
            id: subject.id,
            sub_type: subject.sub_type,
            sub_id: subject.sub_id,
            attrs: subject.attrs,
            limits,
        }
    }
}
