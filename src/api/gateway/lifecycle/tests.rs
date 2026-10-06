use super::*;

#[tokio::test(start_paused = true)]
async fn failover_phases_share_one_absolute_budget() {
    let execution = Execution::new(Duration::from_secs(3), CancellationToken::new());
    execution
        .run(
            "response_headers",
            Duration::from_secs(10),
            tokio::time::sleep(Duration::from_secs(2)),
        )
        .await
        .unwrap();
    let error = execution
        .run(
            "response_headers",
            Duration::from_secs(10),
            tokio::time::sleep(Duration::from_secs(2)),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::GatewayTimeout);
    assert_eq!(error.params["phase"], "total");
}
