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

static AUDIT_SERVICE: once_cell::sync::OnceCell<AuditService> = once_cell::sync::OnceCell::new();

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
            .unwrap_or(1000);
        let (tx, rx) = mpsc::channel::<Audit>(buffer_size);
        let shutdown = Arc::new(Notify::new());
        let handle = tokio::spawn(Self::worker(
            rx,
            batch_size,
            flush_interval,
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
        batch_size: usize,
        flush_interval: Duration,
        shutdown: Arc<Notify>,
    ) {
        let mut buffer = Vec::with_capacity(batch_size);
        let mut interval = tokio::time::interval(flush_interval);

        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Some(audit) => {
                            buffer.push(audit);
                            if buffer.len() >= batch_size {
                                tracing::info!("Processing batch of {} audit records", buffer.len());
                                Self::flush(&mut buffer).await;
                            }
                        }
                        None => {
                            break;
                        }
                    }
                }
                _ = interval.tick() => {
                    if !buffer.is_empty() {
                        tracing::info!("Processing batch of {} audit records on interval", buffer.len());
                        Self::flush(&mut buffer).await;
                    }
                }
                _ = shutdown.notified() => {
                    // Drain remaining messages from channel
                    while let Ok(audit) = rx.try_recv() {
                        buffer.push(audit);
                    }
                    if !buffer.is_empty() {
                        tracing::info!("Flushing {} remaining audit records on shutdown", buffer.len());
                        Self::flush(&mut buffer).await;
                    }
                    break;
                }
            }
        }
    }

    async fn flush(buffer: &mut Vec<Audit>) {
        if buffer.is_empty() {
            return;
        }

        let batch = std::mem::take(buffer);
        let count = batch.len();

        let svc = service();
        match svc.insert_audit_log_bulk(batch.clone()).await {
            Ok(_) => {
                tracing::debug!("Successfully inserted {} audit records", count);
            }
            Err(e) => {
                tracing::error!("Failed to insert audit records: {}", e);
                buffer.extend(batch);
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
