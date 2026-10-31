use super::{
    LimitSpec, OnStoreError,
    graph::CompileError,
    limit::{default_on_store_error, is_default_on_store_error},
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ingress {
    pub limit: String,
    #[serde(default = "default_timeout")]
    pub timeout: String,
    #[serde(
        default = "default_on_store_error",
        skip_serializing_if = "is_default_on_store_error"
    )]
    pub on_store_error: String,
}

fn default_timeout() -> String {
    "250ms".into()
}

impl Default for Ingress {
    fn default() -> Self {
        Self {
            limit: "ingress".into(),
            timeout: default_timeout(),
            on_store_error: default_on_store_error(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompiledIngress {
    pub limit: String,
    pub timeout: Duration,
    pub on_store_error: OnStoreError,
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
            on_store_error: OnStoreError::compile(&self.on_store_error, "ingress.on_store_error")?,
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

#[cfg(test)]
mod store_error_tests {
    use super::*;
    use crate::cfg::{Config, ConfigLoadError, RuntimeConfig};
    use serde_json::json;

    #[test]
    fn traffic_store_failure_modes_compile_and_defaults_preserve_serialization() {
        for kind in ["rate_limit", "quota"] {
            let mut value = serde_json::to_value(Config::default()).unwrap();
            value["limits"]["daily"] =
                json!({"strategy":"quota_tracker", "params":{"limit":100,"period":"day"}});
            value["http"]["policies"]["traffic"] =
                json!({"kind":kind,"limit":if kind == "quota" {"daily"} else {"default"}});
            let config: Config = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(
                config.compile().unwrap().ingress.on_store_error,
                OnStoreError::Allow
            );
            let serialized = serde_json::to_value(config).unwrap();
            assert!(serialized["ingress"].get("on_store_error").is_none());
            assert!(
                serialized["http"]["policies"]["traffic"]
                    .get("on_store_error")
                    .is_none()
            );
            for mode in ["allow", "deny"] {
                value["ingress"]["on_store_error"] = json!(mode);
                value["http"]["policies"]["traffic"]["on_store_error"] = json!(mode);
                let compiled = serde_json::from_value::<Config>(value.clone())
                    .unwrap()
                    .compile()
                    .unwrap();
                let expected = if mode == "deny" {
                    OnStoreError::Deny
                } else {
                    OnStoreError::Allow
                };
                assert_eq!(compiled.ingress.on_store_error, expected);
                let actual = match &compiled.http.policies["traffic"] {
                    crate::graph::PolicyNode::RateLimit { on_store_error, .. }
                    | crate::graph::PolicyNode::Quota { on_store_error, .. } => *on_store_error,
                    _ => unreachable!(),
                };
                assert_eq!(actual, expected);
            }
            for path in [vec!["ingress"], vec!["http", "policies", "traffic"]] {
                let mut invalid = value.clone();
                let mut target = &mut invalid;
                for part in &path {
                    target = &mut target[*part];
                }
                target["on_store_error"] = json!("ignore");
                let yaml = serde_saphyr::to_string(&invalid).unwrap();
                let ConfigLoadError::Compile(error) =
                    RuntimeConfig::from_yaml_str(&yaml).unwrap_err()
                else {
                    panic!("expected compilation error")
                };
                assert_eq!(error.path, format!("{}.on_store_error", path.join(".")));
            }
        }
    }
}
