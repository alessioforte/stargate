use super::{LimitSpec, graph::CompileError};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ingress {
    pub limit: String,
    #[serde(default = "default_timeout")]
    pub timeout: String,
}

fn default_timeout() -> String {
    "250ms".into()
}

impl Default for Ingress {
    fn default() -> Self {
        Self {
            limit: "ingress".into(),
            timeout: default_timeout(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompiledIngress {
    pub limit: String,
    pub timeout: Duration,
}

impl Ingress {
    pub(super) fn compile(
        &self,
        limits: &IndexMap<String, LimitSpec>,
    ) -> Result<CompiledIngress, CompileError> {
        let spec = limits.get(&self.limit).ok_or_else(|| {
            CompileError::new("ingress.limit", "must reference a configured rate limit")
        })?;
        spec.validate_rate()
            .map_err(|message| CompileError::new("ingress.limit", message))?;
        let timeout = tools::parse_duration(&self.timeout)
            .ok()
            .and_then(|value| value.to_std().ok())
            .filter(|value| {
                !value.is_zero() && std::time::Instant::now().checked_add(*value).is_some()
            })
            .ok_or_else(|| {
                CompileError::new(
                    "ingress.timeout",
                    "must be a positive, representable duration",
                )
            })?;
        Ok(CompiledIngress {
            limit: self.limit.clone(),
            timeout,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfg::{Config, RuntimeConfig};

    #[test]
    fn supplied_configs_require_explicit_ingress_and_a_valid_rate_reference() {
        let valid = serde_json::to_value(Config::default()).unwrap();
        for ingress in [
            None,
            Some(serde_json::json!({})),
            Some(serde_json::json!({"limit":"missing"})),
            Some(serde_json::json!({"limit":""})),
            Some(serde_json::json!({"limit":"ingress", "enabled":false})),
        ] {
            let mut value = valid.clone();
            match ingress {
                Some(ingress) => value["ingress"] = ingress,
                None => {
                    value.as_object_mut().unwrap().remove("ingress");
                }
            }
            let yaml = serde_saphyr::to_string(&value).unwrap();
            assert!(RuntimeConfig::from_yaml_str(&yaml).is_err());
        }
        for spec in [
            serde_json::json!({"strategy":"token_bucket", "params":{"capacity":0, "refill_rate":1}}),
            serde_json::json!({"strategy":"token_bucket", "params":{"capacity":1, "refill_rate":0}}),
            serde_json::json!({"strategy":"token_bucket", "params":{"capacity":u64::MAX, "refill_rate":1}}),
            serde_json::json!({"strategy":"quota_tracker", "params":{"limit":100, "period":"hour"}}),
        ] {
            let mut value = valid.clone();
            value["limits"]["ingress"] = spec;
            let config: Config = serde_json::from_value(value).unwrap();
            assert_eq!(config.compile().unwrap_err().path, "ingress.limit");
        }
    }

    #[test]
    fn store_deadline_is_positive_and_the_default_can_boot() {
        let config = Config::default();
        let compiled = config.compile().unwrap();
        assert_eq!(compiled.ingress.limit, "ingress");
        assert_eq!(compiled.ingress.timeout, Duration::from_millis(250));
        assert!(compiled.limits.iter().any(|limit| limit.name == "ingress"));
        for timeout in ["bad", "0s", "-1s", "9223372036854775807s"] {
            let mut config = config.clone();
            config.ingress.timeout = timeout.into();
            assert_eq!(config.compile().unwrap_err().path, "ingress.timeout");
        }
        let mut token_bucket = serde_json::to_value(config).unwrap();
        token_bucket["limits"]["ingress"] = serde_json::json!({"strategy":"token_bucket", "params":{"capacity":100, "refill_rate":10}});
        assert!(
            serde_json::from_value::<Config>(token_bucket)
                .unwrap()
                .compile()
                .is_ok()
        );
    }
}
