use crate::lb::LoadBalancer;
use chrono::Duration;
use std::sync::Arc;
use tracing::info;

pub struct HealthCheck {
    pub lbs: dashmap::DashMap<Duration, Vec<Arc<dyn LoadBalancer + Send + Sync>>>,
    handles: Vec<tokio::task::JoinHandle<()>>,
}

impl HealthCheck {
    pub fn new() -> Self {
        Self {
            lbs: dashmap::DashMap::new(),
            handles: vec![],
        }
    }

    pub fn register(&self, interval: Duration, lb: Arc<dyn LoadBalancer + Send + Sync>) {
        if let Some(mut vec) = self.lbs.get_mut(&interval) {
            vec.push(lb);
        } else {
            self.lbs.insert(interval, vec![lb]);
        }
    }

    pub fn run(&mut self) {
        let lbs = self.lbs.clone();
        info!("Starting liveness probes for {} load balancers", lbs.len());
        for item in lbs.iter() {
            let (interval, lbs) = item.pair();
            let interval = *interval;
            let lbs = lbs.clone();
            let handle = tokio::spawn(async move {
                loop {
                    for lb in &*lbs {
                        lb.health_check().await;
                    }
                    tokio::time::sleep(interval.to_std().unwrap()).await;
                }
            });
            self.handles.push(handle);
        }
    }

    pub fn stop(&mut self) {
        for handle in self.handles.drain(..) {
            handle.abort();
        }
        info!("Upstreams liveness probes stopped");
    }
}
