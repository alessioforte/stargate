use super::graph::CompileError;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Process capacities are fixed at startup; deadlines belong to each snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RuntimeSettings {
    pub primary_concurrency: usize,
    pub mirror_concurrency: usize,
    pub replay_body_bytes: usize,
    pub replay_memory_bytes: usize,
    pub upload_idle_timeout: String,
    pub connect_timeout: String,
    pub response_header_timeout: String,
    pub response_body_idle_timeout: String,
    pub discarded_body_timeout: String,
    pub discarded_body_bytes: usize,
    pub mirror_timeout: String,
    pub request_timeout: String,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            primary_concurrency: 1024,
            mirror_concurrency: 64,
            replay_body_bytes: 2 * 1024 * 1024,
            replay_memory_bytes: 64 * 1024 * 1024,
            upload_idle_timeout: "15s".into(),
            connect_timeout: "5s".into(),
            response_header_timeout: "30s".into(),
            response_body_idle_timeout: "30s".into(),
            discarded_body_timeout: "250ms".into(),
            discarded_body_bytes: 64 * 1024,
            mirror_timeout: "5s".into(),
            request_timeout: "60s".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessBudgets {
    pub primary_concurrency: usize,
    pub mirror_concurrency: usize,
    pub replay_memory_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct CompiledRuntimeSettings {
    pub budgets: ProcessBudgets,
    pub replay_body_bytes: usize,
    pub upload_idle_timeout: Duration,
    pub connect_timeout: Duration,
    pub response_header_timeout: Duration,
    pub response_body_idle_timeout: Duration,
    pub discarded_body_timeout: Duration,
    pub discarded_body_bytes: usize,
    pub mirror_timeout: Duration,
    pub request_timeout: Duration,
}

impl RuntimeSettings {
    pub fn compile(&self) -> Result<CompiledRuntimeSettings, CompileError> {
        for (name, value) in [
            ("primary_concurrency", self.primary_concurrency),
            ("mirror_concurrency", self.mirror_concurrency),
            ("replay_body_bytes", self.replay_body_bytes),
            ("replay_memory_bytes", self.replay_memory_bytes),
            ("discarded_body_bytes", self.discarded_body_bytes),
        ] {
            if value == 0 || value > (u32::MAX as usize).min(usize::MAX >> 3) {
                return Err(CompileError::new(
                    format!("runtime.{name}"),
                    format!(
                        "must be between 1 and {}",
                        (u32::MAX as usize).min(usize::MAX >> 3)
                    ),
                ));
            }
        }
        if self.replay_body_bytes > self.replay_memory_bytes {
            return Err(CompileError::new(
                "runtime.replay_body_bytes",
                "must not exceed replay_memory_bytes",
            ));
        }
        let duration = |name: &str, value: &str| {
            tools::parse_duration(value)
                .ok()
                .and_then(|value| value.to_std().ok())
                .filter(|value| !value.is_zero() && Instant::now().checked_add(*value).is_some())
                .ok_or_else(|| {
                    CompileError::new(
                        format!("runtime.{name}"),
                        "must be a positive, representable duration",
                    )
                })
        };
        Ok(CompiledRuntimeSettings {
            budgets: ProcessBudgets {
                primary_concurrency: self.primary_concurrency,
                mirror_concurrency: self.mirror_concurrency,
                replay_memory_bytes: self.replay_memory_bytes,
            },
            replay_body_bytes: self.replay_body_bytes,
            upload_idle_timeout: duration("upload_idle_timeout", &self.upload_idle_timeout)?,
            connect_timeout: duration("connect_timeout", &self.connect_timeout)?,
            response_header_timeout: duration(
                "response_header_timeout",
                &self.response_header_timeout,
            )?,
            response_body_idle_timeout: duration(
                "response_body_idle_timeout",
                &self.response_body_idle_timeout,
            )?,
            discarded_body_timeout: duration(
                "discarded_body_timeout",
                &self.discarded_body_timeout,
            )?,
            discarded_body_bytes: self.discarded_body_bytes,
            mirror_timeout: duration("mirror_timeout", &self.mirror_timeout)?,
            request_timeout: duration("request_timeout", &self.request_timeout)?,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseMode {
    #[default]
    Finite,
    Stream,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_capacities_and_durations() {
        for field in [
            "primary_concurrency",
            "mirror_concurrency",
            "replay_body_bytes",
            "replay_memory_bytes",
            "discarded_body_bytes",
        ] {
            let mut value = serde_json::to_value(RuntimeSettings::default()).unwrap();
            value[field] = 0.into();
            assert!(
                serde_json::from_value::<RuntimeSettings>(value)
                    .unwrap()
                    .compile()
                    .is_err()
            );
        }
        for field in [
            "upload_idle_timeout",
            "connect_timeout",
            "response_header_timeout",
            "response_body_idle_timeout",
            "discarded_body_timeout",
            "mirror_timeout",
            "request_timeout",
        ] {
            for bad in ["bad", "0s", "-1s", "9223372036854775807s"] {
                let mut value = serde_json::to_value(RuntimeSettings::default()).unwrap();
                value[field] = bad.into();
                assert!(
                    serde_json::from_value::<RuntimeSettings>(value)
                        .unwrap()
                        .compile()
                        .is_err()
                );
            }
        }
        assert!(
            RuntimeSettings {
                replay_memory_bytes: 1,
                ..Default::default()
            }
            .compile()
            .is_err()
        );
    }
}
