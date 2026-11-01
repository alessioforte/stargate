use super::*;

#[tokio::test(start_paused = true)]
async fn quiet_points_expire_after_a_minute_and_repeated_errors_refresh_only_their_window() {
    let health = LimitStoreHealth::default();
    health.failure(LimitPoint::Quota, OnStoreError::Allow, "backend");
    health.failure(LimitPoint::RateLimit, OnStoreError::Deny, "timeout");
    tokio::time::advance(Duration::from_secs(59)).await;
    let points = health.degraded_points();
    assert!(points["quota"]);
    assert!(points["rate_limit"]);
    assert!(!points["admin_principal"]);
    health.success(LimitPoint::IamIngress);
    health.failure(LimitPoint::RateLimit, OnStoreError::Deny, "backend");
    tokio::time::advance(Duration::from_secs(1)).await;
    let points = health.degraded_points();
    assert!(
        !points["quota"],
        "idle quota must expire without another quota request"
    );
    assert!(
        points["rate_limit"],
        "a fresh error extends only its own window"
    );
    tokio::time::advance(Duration::from_secs(59)).await;
    assert!(!health.is_degraded());
    assert!(health.degraded_points().values().all(|value| !value));
}

#[tokio::test(start_paused = true)]
async fn warnings_are_throttled_across_intermittent_failures_and_recovery_logs_are_paired() {
    let mut state = PointHealth::default();
    let now = Instant::now();
    assert!(state.failure(now));
    // Persistent failures refresh the diagnostic window but are not transitions.
    tokio::time::advance(WARNING_INTERVAL).await;
    assert!(
        state.failure(Instant::now()),
        "expired inactivity starts another episode"
    );
    assert!(!state.failure(Instant::now()));
    assert!(state.success());
    assert!(!state.success());
    for _ in 0..50 {
        assert!(!state.failure(Instant::now()));
        assert!(
            !state.success(),
            "a suppressed warning must not create an orphan recovery log"
        );
    }
    tokio::time::advance(WARNING_INTERVAL - Duration::from_nanos(1)).await;
    assert!(!state.failure(Instant::now()));
    assert!(!state.success());
    tokio::time::advance(Duration::from_nanos(1)).await;
    assert!(state.failure(Instant::now()));
    assert!(state.success());
}

#[tokio::test(start_paused = true)]
async fn persistent_failures_do_not_generate_periodic_warnings() {
    let mut state = PointHealth::default();
    assert!(state.failure(Instant::now()));
    for _ in 0..120 {
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(!state.failure(Instant::now()));
        assert!(state.is_degraded(Instant::now()));
    }
    assert!(state.success());
}

#[test]
fn every_named_point_has_independent_storage_and_an_unambiguous_diagnostic_key() {
    let health = LimitStoreHealth::default();
    let points = health.degraded_points();
    assert_eq!(
        points.len(),
        LimitPoint::ALL.len(),
        "point names must be unique"
    );
    for &selected in LimitPoint::ALL {
        health.failure(selected, OnStoreError::Allow, "backend");
        for &point in LimitPoint::ALL {
            assert_eq!(
                health.degraded_points()[point.name()],
                point.name() == selected.name()
            );
        }
        health.success(selected);
    }
    assert!(!health.is_degraded());
}
