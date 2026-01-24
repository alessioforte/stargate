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
use std::time::Duration;
use tokio::sync::mpsc;

static AUDIT_SERVICE: once_cell::sync::OnceCell<AuditService> = once_cell::sync::OnceCell::new();

pub struct AuditService {
    tx: mpsc::Sender<Audit>,
}

impl AuditService {
    pub fn new(batch_size: usize, flush_interval: Duration) -> Self {
        let (tx, rx) = mpsc::channel::<Audit>(1000);
        tokio::spawn(Self::worker(rx, batch_size, flush_interval));
        AuditService { tx }
    }

    pub async fn log(&self, audit: Audit) {
        if let Err(e) = self.tx.send(audit).await {
            tracing::error!("Failed to send audit record: {}", e);
        }
    }

    async fn worker(mut rx: mpsc::Receiver<Audit>, batch_size: usize, flush_interval: Duration) {
        let mut buffer = Vec::with_capacity(batch_size);
        let mut interval = tokio::time::interval(flush_interval);

        loop {
            tokio::select! {
                Some(audit) = rx.recv() => {
                    buffer.push(audit);
                    if buffer.len() >= batch_size {
                        tracing::info!("Processing batch of {} audit records", buffer.len());
                        Self::flush(&mut buffer).await;
                    }
                }
                _ = interval.tick() => {
                    if !buffer.is_empty() {
                        tracing::info!("Processing batch of {} audit records on interval", buffer.len());
                        Self::flush(&mut buffer).await;
                    }
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
