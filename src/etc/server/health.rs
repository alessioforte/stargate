use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::time::{Instant, timeout};

const CHECK_TIMEOUT: Duration = Duration::from_secs(1);

/// Starts unready; initialization and shutdown own the transitions.
#[derive(Clone, Default)]
pub struct Lifecycle(Arc<AtomicBool>);

impl Lifecycle {
    pub fn mark_ready(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn begin_shutdown(&self) {
        self.0.store(false, Ordering::Release);
    }

    pub async fn check_ready(&self, check: impl Future<Output = bool>) -> bool {
        if !self.0.load(Ordering::Acquire) {
            return false;
        }
        // A probe already in flight must observe shutdown too.
        check.await && self.0.load(Ordering::Acquire)
    }
}

pub struct ComponentHealth {
    pub latency_ms: f64,
    pub error: Option<String>,
}

impl ComponentHealth {
    pub fn is_healthy(&self) -> bool {
        self.error.is_none()
    }

    async fn check(check: impl Future<Output = anyhow::Result<()>>) -> Self {
        let started = Instant::now();
        let error = match timeout(CHECK_TIMEOUT, check).await {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error.to_string()),
            Err(_) => Some("Health check timed out after 1s".to_string()),
        };
        Self {
            latency_ms: started.elapsed().as_secs_f64() * 1000.0,
            error,
        }
    }
}

pub struct DependencyHealth {
    pub database: ComponentHealth,
    #[cfg(feature = "redis")]
    pub redis: ComponentHealth,
}

impl DependencyHealth {
    pub fn is_healthy(&self) -> bool {
        let healthy = self.database.is_healthy();
        #[cfg(feature = "redis")]
        let healthy = healthy && self.redis.is_healthy();
        healthy
    }

    async fn check(
        database: impl Future<Output = anyhow::Result<()>>,
        #[cfg(feature = "redis")] redis: impl Future<Output = anyhow::Result<()>>,
    ) -> Self {
        #[cfg(feature = "redis")]
        let (database, redis) = tokio::join!(
            ComponentHealth::check(database),
            ComponentHealth::check(redis),
        );
        #[cfg(not(feature = "redis"))]
        let database = ComponentHealth::check(database).await;

        Self {
            database,
            #[cfg(feature = "redis")]
            redis,
        }
    }
}

/// Shared by readiness probes and authenticated operator diagnostics.
pub async fn check_dependencies() -> DependencyHealth {
    DependencyHealth::check(
        crate::db::ping(),
        #[cfg(feature = "redis")]
        crate::etc::store::ping(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;

    #[tokio::test]
    async fn lifecycle_gates_checks_and_observes_shutdown_during_a_probe() {
        let lifecycle = Lifecycle::default();
        assert!(!lifecycle.check_ready(pending()).await);
        lifecycle.mark_ready();
        assert!(lifecycle.check_ready(async { true }).await);
        assert!(!lifecycle.check_ready(async { false }).await);
        assert!(lifecycle.check_ready(async { true }).await);
        assert!(
            !lifecycle
                .check_ready(async {
                    lifecycle.begin_shutdown();
                    true
                })
                .await
        );
        assert!(!lifecycle.check_ready(pending()).await);
    }

    #[tokio::test]
    async fn database_failure_makes_dependencies_unhealthy() {
        for healthy in [true, false] {
            let result = DependencyHealth::check(
                async {
                    anyhow::ensure!(healthy, "database unavailable");
                    Ok(())
                },
                #[cfg(feature = "redis")]
                async {
                    Ok(())
                },
            )
            .await;
            assert_eq!(result.is_healthy(), healthy);
        }
    }

    #[cfg(feature = "redis")]
    #[tokio::test]
    async fn redis_failure_makes_dependencies_unhealthy_with_a_healthy_database() {
        let result = DependencyHealth::check(async { Ok(()) }, async {
            anyhow::bail!("Redis unavailable")
        })
        .await;
        assert!(result.database.is_healthy());
        assert!(!result.redis.is_healthy());
        assert!(!result.is_healthy());
    }

    #[tokio::test(start_paused = true)]
    async fn hanging_dependencies_share_one_timeout_window() {
        let started = Instant::now();
        let result = DependencyHealth::check(
            pending(),
            #[cfg(feature = "redis")]
            pending(),
        )
        .await;
        assert_eq!(started.elapsed(), CHECK_TIMEOUT);
        assert!(!result.database.is_healthy());
        #[cfg(feature = "redis")]
        assert!(!result.redis.is_healthy());
        assert!(!result.is_healthy());
    }
}
