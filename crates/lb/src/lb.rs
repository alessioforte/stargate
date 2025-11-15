use std::{
    net::IpAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize},
    },
};

use reqwest::Client;

pub struct Upstream {
    pub base_url: String,
    pub alive: Arc<AtomicBool>,
    pub health_check_path: Option<String>, // e.g., "/health"
    pub fail_count: Arc<AtomicUsize>,      // for circuit breaker
}

impl Upstream {
    pub fn new(base_url: String, health_check_path: Option<String>) -> Self {
        Self {
            base_url,
            health_check_path,
            alive: Arc::new(AtomicBool::new(true)),
            fail_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn health_check_url(&self) -> Option<String> {
        self.health_check_path
            .as_ref()
            .map(|path| format!("{}{}", self.base_url, path))
    }
}

pub struct RequestContext {
    pub client_ip: Option<IpAddr>,
    pub path: String,
    pub method: String,
    // pub headers: Option<http::HeaderMap>,
    pub key: Option<String>, // for hashing/stickiness
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
    // fn select<'a>(&self, alive: &'a [&Upstream], context: &RequestContext) -> Option<&'a Upstream>;
    fn select<'a>(
        &self,
        alive: &Vec<&'a Upstream>,
        context: &RequestContext,
    ) -> Option<&'a Upstream>;
    fn name(&self) -> &'static str;
}

pub struct BaseLoadBalancer<S: Strategy> {
    strategy: S,
    upstreams: Vec<Upstream>,
    client: Arc<Client>,
}

impl<S: Strategy + 'static> BaseLoadBalancer<S> {
    pub fn new(strategy: S, upstreams: Vec<Upstream>) -> Arc<Self> {
        Arc::new(Self {
            strategy,
            upstreams,
            client: Arc::new(Client::new()),
        })
    }

    fn alive_upstreams(&self) -> Vec<&Upstream> {
        self.upstreams
            .iter()
            .filter(|upstream| upstream.alive.load(std::sync::atomic::Ordering::Relaxed))
            .collect()
    }
}

#[async_trait::async_trait]
impl<S: Strategy + 'static> LoadBalancer for BaseLoadBalancer<S> {
    fn select(&self, context: &RequestContext) -> Option<&Upstream> {
        let alive = self.alive_upstreams();
        if alive.is_empty() {
            return None;
        }
        self.strategy.select(&alive, context)
    }

    fn name(&self) -> &'static str {
        self.strategy.name()
    }

    fn mark_alive(&self, base_url: &str) {
        if let Some(upstream) = self.upstreams.iter().find(|u| u.base_url == base_url) {
            upstream
                .alive
                .store(true, std::sync::atomic::Ordering::Relaxed);
            upstream
                .fail_count
                .store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn mark_dead(&self, base_url: &str) {
        if let Some(upstream) = self.upstreams.iter().find(|u| u.base_url == base_url) {
            upstream
                .alive
                .store(false, std::sync::atomic::Ordering::Relaxed);
            upstream
                .fail_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    async fn health_check(&self) {
        for upstream in &self.upstreams {
            if let Some(url) = upstream.health_check_url() {
                match self.client.get(&url).send().await {
                    Ok(_) => self.mark_alive(&upstream.base_url),
                    Err(e) => {
                        self.mark_dead(&upstream.base_url);
                        tracing::warn!("Health check failed for {}: {}", upstream.base_url, e);
                    }
                }
            }
        }
    }
}
