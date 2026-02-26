use crate::circuit_breaker::CircuitBreaker;
use std::collections::HashMap;
use std::sync::Arc;

use reqwest::Client;

pub struct Upstream {
    pub base_url: String,
    pub health_check_path: Option<String>,
    pub circuit_breaker: Arc<CircuitBreaker>,
}

impl Upstream {
    pub fn new(base_url: String, health_check_path: Option<String>) -> Self {
        // Defaults: open after 3 consecutive failures, 30s cooldown
        Self::with_circuit_breaker(base_url, health_check_path, 3, 30)
    }

    pub fn with_circuit_breaker(
        base_url: String,
        health_check_path: Option<String>,
        fail_threshold: usize,
        cooldown_secs: u64,
    ) -> Self {
        Self {
            base_url,
            health_check_path,
            circuit_breaker: Arc::new(CircuitBreaker::new(fail_threshold, cooldown_secs)),
        }
    }

    pub fn is_available(&self) -> bool {
        self.circuit_breaker.is_available()
    }

    pub fn health_check_url(&self) -> Option<String> {
        self.health_check_path
            .as_ref()
            .map(|path| format!("{}{}", self.base_url, path))
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
    index: HashMap<String, usize>,
    client: Arc<Client>,
}

impl<S: Strategy + 'static> BaseLoadBalancer<S> {
    pub fn new(strategy: S, upstreams: Vec<Upstream>) -> Arc<Self> {
        let index = upstreams
            .iter()
            .enumerate()
            .map(|(i, u)| (u.base_url.clone(), i))
            .collect();

        let client = match Client::builder()
            .connect_timeout(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(2))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to build HTTP client for health checks: {}", e);
                Client::new() // fallback to default client
            }
        };

        Arc::new(Self {
            strategy,
            upstreams,
            index,
            client: Arc::new(client),
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

    async fn health_check(&self) {
        let mut set = tokio::task::JoinSet::new();
        for upstream in &self.upstreams {
            if let Some(url) = upstream.health_check_url() {
                let client = Arc::clone(&self.client);
                let cb = Arc::clone(&upstream.circuit_breaker);
                let base_url = upstream.base_url.clone();
                set.spawn(async move {
                    match client.get(&url).send().await {
                        Ok(resp) if resp.status().is_success() => {
                            cb.record_success();
                        }
                        Ok(resp) => {
                            cb.record_failure();
                            tracing::warn!(
                                "Health check failed for {}: status {}",
                                base_url,
                                resp.status()
                            );
                        }
                        Err(e) => {
                            cb.record_failure();
                            tracing::warn!("Health check failed for {}: {}", base_url, e);
                        }
                    }
                });
            }
        }
        while set.join_next().await.is_some() {}
    }
}
