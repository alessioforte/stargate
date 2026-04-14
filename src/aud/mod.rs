// ## **🏗️ Audit Service Architecture** ##

//        ┌─────────────────────────────────────────┐
//        │  AuditService (Singleton)               │
//        │  - Event Bus Centralized                │
//        │  - Automatic buffering of events        │
//        │  - Flush every 5 seconds                │
//        └───────┬─────────────────────────────────┘
//                │
//        ┌───────┴───────┬─────────────┬─────────────┐
//        ▼               ▼             ▼             ▼
//   Middleware      Repository      Service     Business Logic
//     (Auto)       (Semi-Auto)     (Explicit)     (Explicit)

pub mod audit;

use crate::db::service;
use db::{Transaction, ent::Audit};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, mpsc};
use tokio::time::Instant;

static AUDIT_SERVICE: once_cell::sync::OnceCell<AuditService> = once_cell::sync::OnceCell::new();
const DEFAULT_AUDIT_BUFFER_SIZE: usize = 1000;

pub struct AuditService {
    tx: mpsc::Sender<Audit>,
    shutdown: Arc<Notify>,
    worker_handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl AuditService {
    pub fn new(batch_size: usize, flush_interval: Duration) -> Self {
        let buffer_size: usize = std::env::var("AUDIT_BUFFER_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|size: &usize| *size > 0)
            .unwrap_or(DEFAULT_AUDIT_BUFFER_SIZE);
        let max_pending: usize = std::env::var("AUDIT_MAX_PENDING")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|size: &usize| *size > 0)
            .unwrap_or(buffer_size);
        let (tx, rx) = mpsc::channel::<Audit>(buffer_size);
        let shutdown = Arc::new(Notify::new());
        let handle = tokio::spawn(Self::worker(
            rx,
            batch_size.min(max_pending),
            flush_interval,
            max_pending,
            shutdown.clone(),
        ));
        AuditService {
            tx,
            shutdown,
            worker_handle: tokio::sync::Mutex::new(Some(handle)),
        }
    }

    pub fn log(&self, audit: Audit) {
        match self.tx.try_send(audit) {
            Ok(_) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!("Audit channel full, dropping event");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::error!("Audit channel closed");
            }
        }
    }

    async fn worker(
        mut rx: mpsc::Receiver<Audit>,
        flush_threshold: usize,
        flush_interval: Duration,
        max_pending: usize,
        shutdown: Arc<Notify>,
    ) {
        let mut buffer = Vec::with_capacity(flush_threshold);
        let mut interval = tokio::time::interval(flush_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut next_retry_at = None;

        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Some(audit) => {
                            Self::push_pending(&mut buffer, audit, max_pending);
                            if buffer.len() >= flush_threshold && Self::retry_due(next_retry_at) {
                                tracing::info!("Processing batch of {} audit records", buffer.len());
                                next_retry_at =
                                    Self::flush_with_backoff(&mut buffer, flush_interval, max_pending).await;
                            }
                        }
                        None => {
                            break;
                        }
                    }
                }
                _ = interval.tick() => {
                    if !buffer.is_empty() && Self::retry_due(next_retry_at) {
                        tracing::info!("Processing batch of {} audit records on interval", buffer.len());
                        next_retry_at =
                            Self::flush_with_backoff(&mut buffer, flush_interval, max_pending).await;
                    }
                }
                _ = shutdown.notified() => {
                    // Drain remaining messages from channel
                    while let Ok(audit) = rx.try_recv() {
                        Self::push_pending(&mut buffer, audit, max_pending);
                    }
                    if !buffer.is_empty() {
                        tracing::info!("Flushing {} remaining audit records on shutdown", buffer.len());
                        if !Self::flush(&mut buffer, max_pending).await {
                            let dropped = buffer.len();
                            tracing::error!(
                                dropped,
                                "Failed to flush audit records during shutdown; dropping remaining buffered events"
                            );
                            buffer.clear();
                        }
                    }
                    break;
                }
            }
        }
    }

    fn retry_due(next_retry_at: Option<Instant>) -> bool {
        match next_retry_at {
            Some(deadline) => Instant::now() >= deadline,
            None => true,
        }
    }

    fn push_pending(buffer: &mut Vec<Audit>, audit: Audit, max_pending: usize) {
        if buffer.len() >= max_pending {
            tracing::warn!(
                max_pending,
                "Audit retry buffer full, dropping newest event"
            );
            return;
        }

        buffer.push(audit);
    }

    fn restore_failed_batch(buffer: &mut Vec<Audit>, mut batch: Vec<Audit>, max_pending: usize) {
        let available = max_pending.saturating_sub(buffer.len());
        let incoming = batch.len();

        if incoming > available {
            batch.truncate(available);
        }

        let restored = batch.len();
        let dropped = incoming.saturating_sub(restored);
        buffer.extend(batch);

        if dropped > 0 {
            tracing::error!(
                dropped,
                restored,
                pending = buffer.len(),
                max_pending,
                "Audit retry buffer reached capacity after a flush failure; dropping newest pending events"
            );
        }
    }

    async fn flush_with_backoff(
        buffer: &mut Vec<Audit>,
        retry_backoff: Duration,
        max_pending: usize,
    ) -> Option<Instant> {
        if Self::flush(buffer, max_pending).await {
            None
        } else {
            Some(Instant::now() + retry_backoff)
        }
    }

    async fn flush(buffer: &mut Vec<Audit>, max_pending: usize) -> bool {
        if buffer.is_empty() {
            return true;
        }

        let batch = std::mem::take(buffer);
        let count = batch.len();

        let svc = service();
        match svc.insert_audit_log_bulk(batch.clone()).await {
            Ok(_) => {
                tracing::debug!("Successfully inserted {} audit records", count);
                true
            }
            Err(e) => {
                tracing::error!(count, max_pending, "Failed to insert audit records: {}", e);
                Self::restore_failed_batch(buffer, batch, max_pending);
                false
            }
        }
    }
}

pub fn init() {
    let batch_size = 1000;
    let flush_interval = Duration::from_secs(5);
    let service = AuditService::new(batch_size, flush_interval);
    if AUDIT_SERVICE.set(service).is_err() {
        tracing::error!("Failed to set the AuditService instance");
    }
}

pub fn get_audit_service() -> Option<&'static AuditService> {
    AUDIT_SERVICE.get()
}

pub async fn shutdown() {
    if let Some(service) = AUDIT_SERVICE.get() {
        service.shutdown.notify_one();
        let mut handle = service.worker_handle.lock().await;
        if let Some(h) = handle.take() {
            let _ = h.await;
        }
        tracing::info!("Audit service shut down");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use db::ent::{ActionType, ActorType};

    fn audit(id: &str) -> Audit {
        Audit {
            id: id.to_string(),
            timestamp: Utc::now(),
            actor_type: ActorType::System,
            actor_id: None,
            action: ActionType::Create,
            resource: None,
            resource_id: None,
            request_id: None,
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn push_pending_drops_new_events_once_capacity_is_reached() {
        let mut buffer = vec![audit("a1"), audit("a2")];

        AuditService::push_pending(&mut buffer, audit("a3"), 2);

        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer[0].id, "a1");
        assert_eq!(buffer[1].id, "a2");
    }

    #[test]
    fn restore_failed_batch_preserves_oldest_pending_events() {
        let mut buffer = vec![audit("a0")];
        let batch = vec![audit("a1"), audit("a2"), audit("a3")];

        AuditService::restore_failed_batch(&mut buffer, batch, 3);

        let ids = buffer
            .iter()
            .map(|audit| audit.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["a0", "a1", "a2"]);
    }
}
