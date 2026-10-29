use super::*;

#[tokio::test(start_paused = true)]
async fn failover_phases_share_one_absolute_budget() {
    let execution = Execution::new(Duration::from_secs(3), CancellationToken::new());
    execution
        .run(
            "response_headers",
            Some(Duration::from_secs(10)),
            tokio::time::sleep(Duration::from_secs(2)),
        )
        .await
        .unwrap();
    let error = execution
        .run(
            "response_headers",
            Some(Duration::from_secs(10)),
            tokio::time::sleep(Duration::from_secs(2)),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::GatewayTimeout);
    assert_eq!(error.params["phase"], "total");
}

#[tokio::test(start_paused = true)]
async fn updated_deadlines_can_extend_or_disable_the_initial_budget() {
    for disabled in [false, true] {
        let execution = Execution::new(Duration::from_secs(1), CancellationToken::new());
        let (updates, deadlines) = tokio::sync::watch::channel(execution.deadline);
        execution
            .run_with_deadline_updates(deadlines, async {
                tokio::time::sleep(Duration::from_millis(500)).await;
                updates.send_replace((!disabled).then(|| Instant::now() + Duration::from_secs(3)));
                tokio::time::sleep(Duration::from_secs(2)).await;
            })
            .await
            .unwrap();
    }
}
