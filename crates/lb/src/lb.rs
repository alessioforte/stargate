use crate::circuit_breaker::CircuitBreaker;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use reqwest::Client;

#[derive(Debug, Clone)]
pub enum HealthProbe {
    Http { path: String },
    Grpc { service: String },
}

pub struct Upstream {
    pub base_url: String,
    pub health_check: Option<HealthProbe>,
    pub circuit_breaker: Arc<CircuitBreaker>,
}

impl Upstream {
    pub fn new(base_url: String, health_check: Option<HealthProbe>) -> Self {
        // Defaults: open after 3 consecutive failures, 30s cooldown
        Self::with_circuit_breaker(base_url, health_check, 3, 30)
    }

    pub fn with_circuit_breaker(
        base_url: String,
        health_check: Option<HealthProbe>,
        fail_threshold: usize,
        cooldown_secs: u64,
    ) -> Self {
        Self {
            base_url,
            health_check,
            circuit_breaker: Arc::new(CircuitBreaker::new(fail_threshold, cooldown_secs)),
        }
    }

    pub fn is_available(&self) -> bool {
        self.circuit_breaker.is_available()
    }
}

pub struct RequestContext<'a> {
    pub client_ip: &'a str,
    pub path: &'a str,
    pub method: &'a str,
    // pub headers: Option<http::HeaderMap>,
    pub key: Option<&'a str>,
}

#[async_trait::async_trait]
pub trait LoadBalancer {
    fn select(&self, context: &RequestContext) -> Option<&Upstream>;
    fn name(&self) -> &'static str;
    fn mark_alive(&self, base_url: &str);
    fn mark_dead(&self, base_url: &str);
    fn set_health_client(&self, client: Client);

    async fn health_check(&self);
}

pub trait Strategy: Send + Sync {
    fn select<'a>(
        &self,
        upstreams: &'a [Upstream],
        context: &RequestContext,
    ) -> Option<&'a Upstream>;
    fn name(&self) -> &'static str;
}

pub struct BaseLoadBalancer<S: Strategy> {
    strategy: S,
    upstreams: Vec<Upstream>,
    // ahash keyed by base_url: mark_alive/mark_dead hit this per request once
    // live proxy results are wired in, so the lookup stays off the critical path.
    index: HashMap<String, usize, ahash::RandomState>,
    client: OnceLock<Client>,
}

impl<S: Strategy + 'static> BaseLoadBalancer<S> {
    pub fn new(strategy: S, upstreams: Vec<Upstream>) -> Arc<Self> {
        let index: HashMap<String, usize, ahash::RandomState> = upstreams
            .iter()
            .enumerate()
            .map(|(i, u)| (u.base_url.clone(), i))
            .collect();

        Arc::new(Self {
            strategy,
            upstreams,
            index,
            client: OnceLock::new(),
        })
    }
}

#[async_trait::async_trait]
impl<S: Strategy + 'static> LoadBalancer for BaseLoadBalancer<S> {
    fn select(&self, context: &RequestContext) -> Option<&Upstream> {
        self.strategy.select(&self.upstreams, context)
    }

    fn name(&self) -> &'static str {
        self.strategy.name()
    }

    fn mark_alive(&self, base_url: &str) {
        if let Some(&idx) = self.index.get(base_url) {
            self.upstreams[idx].circuit_breaker.record_success();
        }
    }

    fn mark_dead(&self, base_url: &str) {
        if let Some(&idx) = self.index.get(base_url) {
            self.upstreams[idx].circuit_breaker.record_failure();
        }
    }

    fn set_health_client(&self, client: Client) {
        self.client
            .set(client)
            .expect("Health probe client must be configured only once");
    }

    async fn health_check(&self) {
        let mut set = tokio::task::JoinSet::new();
        for upstream in &self.upstreams {
            if let Some(probe) = &upstream.health_check {
                let Some(client) = self.client.get().cloned() else {
                    tracing::error!(upstream = %upstream.base_url, "Health probe client has not been configured");
                    upstream.circuit_breaker.record_failure();
                    continue;
                };
                let cb = Arc::clone(&upstream.circuit_breaker);
                let base_url = upstream.base_url.clone();
                let probe = probe.clone();
                set.spawn(async move {
                    let healthy = match probe {
                        HealthProbe::Http { path } => match client
                            .get(format!("{base_url}{path}"))
                            .send()
                            .await
                        {
                            Ok(response) if response.status().is_success() => true,
                            Ok(response) => {
                                tracing::warn!(
                                    "Health check failed for {}: status {}",
                                    base_url,
                                    response.status()
                                );
                                false
                            }
                            Err(error) => {
                                tracing::warn!("Health check failed for {}: {}", base_url, error);
                                false
                            }
                        },
                        HealthProbe::Grpc { service } => {
                            let healthy =
                                crate::grpc_health::check(&client, &base_url, &service).await;
                            if !healthy {
                                tracing::warn!("gRPC health check failed for {}", base_url);
                            }
                            healthy
                        }
                    };
                    if healthy {
                        cb.record_success();
                    } else {
                        cb.record_failure();
                    }
                });
            }
        }
        while set.join_next().await.is_some() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn probes_without_a_prepared_client_mark_targets_unhealthy() {
        for probe in [
            HealthProbe::Http {
                path: "/health".into(),
            },
            HealthProbe::Grpc {
                service: String::new(),
            },
        ] {
            let upstream =
                Upstream::with_circuit_breaker("https://localhost:1".into(), Some(probe), 1, 3600);
            let balancer = BaseLoadBalancer::new(crate::RoundRobin::new(), vec![upstream]);
            balancer.health_check().await;
            let context = RequestContext {
                client_ip: "127.0.0.1",
                path: "/",
                method: "GET",
                key: None,
            };
            assert!(balancer.select(&context).is_none());
        }
    }
}
