use super::*;

#[test]
fn sync_interval_defaults_to_thirty_seconds_and_accepts_positive_integers() {
    assert_eq!(sync_interval(None).unwrap(), Duration::from_secs(30));
    for seconds in [1, 30, 300] {
        assert_eq!(
            sync_interval(Some(&seconds.to_string())).unwrap(),
            Duration::from_secs(seconds)
        );
    }
}

#[test]
fn sync_interval_rejects_invalid_or_unrepresentable_values() {
    for value in [
        "",
        "0",
        "-1",
        "1.5",
        "false",
        " 1",
        "1s",
        "18446744073709551615",
        "18446744073709551616",
    ] {
        let error = sync_interval(Some(value)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains(INTERVAL_ENV));
    }
}

#[test]
fn revision_comparison_skips_active_documents_and_applies_changed_documents() {
    let mut state = RevisionState::default();
    for trigger in [Trigger::Notification, Trigger::Tick] {
        assert!(!state.should_apply("a", "a", trigger));
        assert!(state.should_apply("a", "b", trigger));
    }
    state.applied();
    assert!(!state.should_apply("b", "b", Trigger::Notification));
}

#[test]
fn failed_revision_waits_for_a_tick_or_new_revision_and_logs_once() {
    let mut state = RevisionState::default();
    assert!(state.failed("b"));
    assert!(!state.should_apply("a", "b", Trigger::Notification));
    assert!(state.should_apply("a", "b", Trigger::Tick));
    assert!(!state.failed("b"));
    assert!(state.should_apply("a", "c", Trigger::Notification));
    assert!(state.failed("c"));
    state.applied();
    assert!(!state.should_apply("c", "c", Trigger::Tick));
    assert!(state.should_apply("c", "d", Trigger::Notification));
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "requires dedicated CONFIG_SYNC_TEST_DATABASE_URL and CONFIG_SYNC_TEST_REDIS_URL services"]
async fn replicas_converge_on_notifications_and_periodic_checks_and_shutdown_cleanly() {
    use crate::data::db::{save_config_document, with_test_service};
    use crate::gate::{
        save_gateway_config,
        snap::{config::prepare_gateway_config, reload::apply_policy_document},
    };
    use ::db::{
        DbStore,
        ent::{
            ConfigDocument, ConfigKind, ConfigSaveOutcome, ConfigSource, TrustedAuditBoundary,
            TrustedAuditContext, TrustedBackgroundActor,
        },
    };
    use gate::{PolicySnapshot, cfg::Config};
    use tokio::sync::Notify;

    fn system() -> TrustedAuditContext {
        TrustedAuditContext::background(
            TrustedAuditBoundary::control_plane(),
            TrustedBackgroundActor::system(None),
        )
    }

    fn configuration(label: &str) -> Config {
        let mut value = serde_json::to_value(Config::default()).unwrap();
        value["http"] = serde_json::json!({
            "services": {"ready": {"kind": "direct_response", "status": 200, "body": {"text": label}}},
            "routers": {"ready": {"match": {"path": {"prefix": "/"}}, "service": "ready"}},
        });
        serde_json::from_value(value).unwrap()
    }

    async fn write(
        svc: &::db::service::Service,
        kind: ConfigKind,
        content: String,
    ) -> ConfigDocument {
        let expected = svc
            .get_active_config_document(kind)
            .await
            .unwrap()
            .map(|document| document.revision);
        match svc
            .save_config_document(kind, content, expected, ConfigSource::CliImport, system())
            .await
            .unwrap()
        {
            ConfigSaveOutcome::Saved(document) | ConfigSaveOutcome::Unchanged(document) => document,
            other => panic!("unexpected concurrent test write: {other:?}"),
        }
    }

    async fn wait_for(gate: &Gate, revision: &str, policies: bool) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let active = if policies {
                    gate.policy_snapshot.load().revision.clone()
                } else {
                    gate.snapshot().config_revision.clone()
                };
                if active == revision {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("replica must converge before the long periodic interval");
    }

    async fn stop_task(stop: Arc<Notify>, task: tokio::task::JoinHandle<()>) {
        stop.notify_one();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("shutdown must cancel an idle listener")
            .expect("synchronizer must not panic");
    }

    let database_url = std::env::var("CONFIG_SYNC_TEST_DATABASE_URL")
        .expect("dedicated PostgreSQL test database required");
    let redis_url =
        std::env::var("CONFIG_SYNC_TEST_REDIS_URL").expect("dedicated Redis test service required");
    let svc = ::db::service::init(&database_url).await.unwrap();
    svc.migrate().await.unwrap();
    let store = Arc::new(store::RedisStore::new(&redis_url).await.unwrap());
    let label = ulid::Ulid::generate().to_string();
    let initial = write(
        &svc,
        ConfigKind::Gateway,
        configuration(&label).to_yaml_string().unwrap(),
    )
    .await;
    write(&svc, ConfigKind::Policies, String::new()).await;
    let first = Arc::new(
        Gate::new(
            store.clone(),
            prepare_gateway_config(&initial.content).unwrap(),
            PolicySnapshot::default(),
        )
        .unwrap(),
    );
    let second = Arc::new(
        Gate::new(
            store,
            prepare_gateway_config(&initial.content).unwrap(),
            PolicySnapshot::default(),
        )
        .unwrap(),
    );
    with_test_service(svc.clone(), async {
        // The first catch-up activation is the barrier proving that LISTEN and the initial tick completed.
        let barrier = write(
            &svc,
            ConfigKind::Gateway,
            configuration("notification barrier")
                .to_yaml_string()
                .unwrap(),
        )
        .await;
        super::super::reload::apply_gateway_config(&first, &barrier.content).unwrap();
        let stop = Arc::new(Notify::new());
        let task = tokio::spawn(with_test_service(
            svc.clone(),
            run(
                second.clone(),
                stop.clone(),
                Wakeups::new(Duration::from_secs(30)),
            ),
        ));
        wait_for(&second, &barrier.revision, false).await;

        let request = ::db::ent::TrustedAuditRequest::from_http(
            ulid::Ulid::generate().to_string(),
            None,
            None,
            None,
        );
        let admin = TrustedAuditContext::admin_control_plane(
            ::db::ent::TrustedAdminActor::admin("01JZ000000000000000000000A"),
            request,
        );
        let (saved, _) = save_gateway_config(
            first.clone(),
            configuration("notified"),
            barrier.revision,
            admin,
        )
        .await
        .unwrap();
        wait_for(&second, &saved.revision, false).await;
        assert_eq!(
            first.snapshot().config_revision,
            second.snapshot().config_revision
        );
        let content = "ALLOW user FOR \"propagated:READ\";";
        let current = svc
            .get_active_config_document(ConfigKind::Policies)
            .await
            .unwrap()
            .unwrap();
        let policy = save_config_document(
            ConfigKind::Policies,
            content.to_string(),
            Some(current.revision),
            ConfigSource::CliImport,
            system(),
        )
        .await
        .unwrap();
        let ConfigSaveOutcome::Saved(policy) = policy else {
            panic!("new policy version")
        };
        apply_policy_document(&first, &policy.content).unwrap();
        wait_for(&second, &policy.revision, true).await;
        stop_task(stop, task).await;

        // A live connection without LISTEN simulates lost notifications and isolates timer recovery.
        let mut silent_listener = svc.config_listener().await.unwrap();
        silent_listener
            .unlisten(::db::CONFIG_CHANNEL)
            .await
            .unwrap();
        let barrier = write(
            &svc,
            ConfigKind::Gateway,
            configuration("periodic barrier").to_yaml_string().unwrap(),
        )
        .await;
        let stop = Arc::new(Notify::new());
        let task = tokio::spawn(with_test_service(
            svc.clone(),
            run(
                second.clone(),
                stop.clone(),
                Wakeups {
                    ticks: tokio::time::interval(Duration::from_millis(200)),
                    listener: Some(silent_listener),
                },
            ),
        ));
        wait_for(&second, &barrier.revision, false).await;
        let saved = write(
            &svc,
            ConfigKind::Gateway,
            configuration("periodic update").to_yaml_string().unwrap(),
        )
        .await;
        wait_for(&second, &saved.revision, false).await;
        stop_task(stop, task).await;
    })
    .await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn edge_timer_applies_external_gateway_and_policy_writes_to_the_same_sqlite_file() {
    use crate::data::db::with_test_service;
    use ::db::{
        DbStore,
        ent::{
            ConfigKind, ConfigSaveOutcome, ConfigSource, TrustedAuditBoundary, TrustedAuditContext,
            TrustedBackgroundActor,
        },
    };
    use gate::cfg::Config;
    use tokio::sync::Notify;

    let directory =
        std::env::temp_dir().join(format!("stargate-edge-sync-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&directory).unwrap();
    let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
    let reader = ::db::service::init(&url).await.unwrap();
    reader.migrate().await.unwrap();
    let writer = ::db::service::init(&url).await.unwrap();
    let initial = Config::default();
    let gate = Arc::new(crate::gate::test_support::gate(initial.clone()));

    async fn external_write(
        writer: &::db::service::Service,
        kind: ConfigKind,
        content: String,
    ) -> String {
        let expected = writer
            .get_active_config_document(kind)
            .await
            .unwrap()
            .map(|document| document.revision);
        let context = TrustedAuditContext::background(
            TrustedAuditBoundary::control_plane(),
            TrustedBackgroundActor::system(None),
        );
        let saved = writer
            .save_config_document(kind, content, expected, ConfigSource::CliImport, context)
            .await
            .unwrap();
        match saved {
            ConfigSaveOutcome::Saved(document) | ConfigSaveOutcome::Unchanged(document) => {
                document.revision
            }
            other => panic!("unexpected external write: {other:?}"),
        }
    }

    async fn wait_for(gate: &Gate, kind: ConfigKind, revision: &str) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let active = match kind {
                    ConfigKind::Gateway => gate.snapshot().config_revision.clone(),
                    ConfigKind::Policies => gate.policy_snapshot.load().revision.clone(),
                };
                if active == revision {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("edge must activate external writes on the next tick");
    }

    external_write(
        &writer,
        ConfigKind::Gateway,
        initial.to_yaml_string().unwrap(),
    )
    .await;
    external_write(&writer, ConfigKind::Policies, String::new()).await;
    let stop = Arc::new(Notify::new());
    let task = tokio::spawn(with_test_service(
        reader,
        run(
            gate.clone(),
            stop.clone(),
            Wakeups::new(Duration::from_millis(30)),
        ),
    ));
    let mut changed = initial;
    for timeout in ["300ms", "400ms"] {
        changed.ingress.timeout = timeout.to_string();
        let revision = external_write(
            &writer,
            ConfigKind::Gateway,
            changed.to_yaml_string().unwrap(),
        )
        .await;
        wait_for(&gate, ConfigKind::Gateway, &revision).await;
    }
    assert_eq!(gate.snapshot().version, 2);
    let revision = external_write(
        &writer,
        ConfigKind::Policies,
        "ALLOW user FOR \"edge-import:READ\";".to_string(),
    )
    .await;
    wait_for(&gate, ConfigKind::Policies, &revision).await;
    stop.notify_one();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    drop(writer);
    std::fs::remove_dir_all(directory).unwrap();
}
