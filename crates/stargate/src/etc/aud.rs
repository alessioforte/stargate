// ## **🏗️ Audit Service Architecture** ##

// ┌─────────────────────────────────────────┐
// │  AuditService (Singleton)               │
// │  - Event Bus Centralized                │
// │  - Automatic buffering of events        │
// │  - Flush every 5 seconds                │
// └──────────────┬──────────────────────────┘
//                │
//        ┌───────┴───────┬─────────────┬─────────────┐
//        ▼               ▼             ▼             ▼
//   Middleware      Repository      Service     Business Logic
//     (Auto)       (Semi-Auto)     (Explicit)     (Explicit)

use db::ent::Audit;
use std::time::Duration;
use tokio::sync::mpsc;

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

        // let batch = buffer.drain(..).collect::<Vec<_>>();
        let batch = std::mem::take(buffer);
        let count = buffer.len();
    }
}

// // ESEMPIO 3: Macro per ridurre ulteriormente il boilerplate (opzionale)
// #[macro_export]
// macro_rules! audit {
//     ($ctx:expr, $event:expr) => {
//         let _ = $ctx.buffer.log($event.build_with_context($ctx));
//     };
// }

// // Uso: audit!(ctx, event);
