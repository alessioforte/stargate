//! Per-task accounting of commands issued to the state store.
//!
//! A caller wraps the part of a request it wants to attribute in
//! [`count_commands`]; backends report every request they send to the store with
//! [`command_issued`], whether it is one command, a script call or a whole
//! pipeline. Reports outside a counting scope are ignored, so an unwatched
//! request pays one thread-local read per command.
//! Scopes nest: an outer counter also sees the commands of an inner one, which
//! reports only its own share.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

tokio::task_local! {
    static ISSUED: Arc<AtomicU32>;
}

/// Runs `future`, returning its output and the number of commands it issued.
///
/// Commands issued by tasks spawned inside `future` are not counted: a task
/// local counter follows the future, which is what per-request accounting
/// needs on the inline request path.
pub async fn count_commands<F: Future>(future: F) -> (F::Output, u32) {
    if let Ok(counter) = ISSUED.try_with(Arc::clone) {
        let before = counter.load(Ordering::Relaxed);
        let output = future.await;
        let commands = counter.load(Ordering::Relaxed).saturating_sub(before);
        return (output, commands);
    }
    let counter = Arc::new(AtomicU32::new(0));
    let scope = counter.clone();
    ISSUED
        .scope(counter, async move {
            let output = future.await;
            (output, scope.load(Ordering::Relaxed))
        })
        .await
}

/// Records one command issued by the current task.
pub fn command_issued() {
    add(1);
}

fn add(count: u32) {
    let _ = ISSUED.try_with(|counter| counter.fetch_add(count, Ordering::Relaxed));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn counts_only_commands_sent_inside_the_scope() {
        command_issued();
        let (result, commands) = count_commands(async {
            command_issued();
            command_issued();
            command_issued();
            command_issued();
            tokio::task::yield_now().await;
            command_issued();
            "done"
        })
        .await;
        assert_eq!(result, "done");
        assert_eq!(commands, 5);
        command_issued();
    }

    #[tokio::test]
    async fn nested_scopes_report_their_own_share_and_keep_the_outer_total() {
        let (outer, outer_commands) = count_commands(async {
            command_issued();
            let (inner, inner_commands) = count_commands(async {
                command_issued();
                command_issued();
            })
            .await;
            assert_eq!((inner, inner_commands), ((), 2));
            command_issued();
        })
        .await;
        assert_eq!((outer, outer_commands), ((), 4));
    }

    #[tokio::test]
    async fn spawned_tasks_do_not_add_to_the_request_count() {
        let (_, commands) = count_commands(async {
            command_issued();
            tokio::spawn(async { command_issued() }).await.unwrap();
        })
        .await;
        assert_eq!(commands, 1);
    }
}
