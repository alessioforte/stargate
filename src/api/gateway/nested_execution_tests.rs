use super::*;
use crate::api::gateway::{execution::execute_plan_with_request, planner::build_execution_plan};
use http::Request;
use serde_json::{Value, json};

fn runtime_for(upstreams: &[(&str, &Upstream)], services: Value) -> Arc<RuntimeSnapshot> {
    let mut value = serde_json::to_value(Config::default()).unwrap();
    value["runtime"] =
        json!({"replay_body_bytes":8,"replay_memory_bytes":32,"request_timeout":"2s"});
    let upstreams = upstreams
        .iter()
        .map(|(name, upstream)| {
            (
                (*name).to_owned(),
                json!({"targets":[{"url":upstream.url}]}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    value["http"] = json!({"upstreams":upstreams,"services":services});
    test_support::runtime(RuntimeConfig::from_raw(serde_json::from_value(value).unwrap()).unwrap())
}

async fn execute(runtime: &Arc<RuntimeSnapshot>, kind: ctx::DispatchKind) -> Response {
    let plan = build_execution_plan(&runtime.core.graph, "root", "request-id").unwrap();
    execute_plan_from_replay(runtime, &plan, &replay(Method::GET), &state(runtime), kind)
        .await
        .unwrap()
}

fn make_unavailable(runtime: &RuntimeSnapshot, service: &str, upstream: &Upstream) {
    for _ in 0..3 {
        runtime.core.balancers[service].mark_dead(&upstream.url);
    }
}

async fn wait_for_mirrors(runtime: &RuntimeSnapshot) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while runtime.resources.available().1 != runtime.settings.budgets.mirror_concurrency {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn fallback_child_status_rules_apply_only_after_entering_the_child() {
    for kind in [ctx::DispatchKind::Primary, ctx::DispatchKind::Shadow] {
        for status in [404, 503] {
            let primary = Upstream::start(Outcome::Status(status)).await;
            let child = Upstream::start(Outcome::Status(404)).await;
            let backup = Upstream::start(Outcome::Status(201)).await;
            let runtime = runtime_for(
                &[
                    ("primary", &primary),
                    ("child", &child),
                    ("backup", &backup),
                ],
                json!({
                    "primary":{"kind":"load_balancer","upstream":"primary"},
                    "child":{"kind":"load_balancer","upstream":"child"},
                    "backup":{"kind":"load_balancer","upstream":"backup"},
                    "child-failover":{"kind":"failover","service":"child","failovers":["backup"],"on_status":[404]},
                    "root":{"kind":"failover","service":"primary","failovers":["child-failover"],"on_status":[503]}
                }),
            );
            let response = execute(&runtime, kind).await;
            assert_eq!(
                response.status().as_u16(),
                if status == 404 { 404 } else { 201 }
            );
            assert_eq!(primary.records().len(), 1);
            assert_eq!(child.records().len(), usize::from(status == 503));
            assert_eq!(backup.records().len(), usize::from(status == 503));
        }
    }
}

#[tokio::test]
async fn a_primary_child_finishes_under_its_own_rules_before_the_parent_retries() {
    for (first, child_final, expected, child_calls, parent_calls) in [
        (503, 200, 201, 0, 1),
        (404, 503, 201, 1, 1),
        (404, 200, 200, 1, 0),
    ] {
        let primary = Upstream::start(Outcome::Status(first)).await;
        let child_backup = Upstream::start(Outcome::Status(child_final)).await;
        let parent_backup = Upstream::start(Outcome::Status(201)).await;
        let runtime = runtime_for(
            &[
                ("primary", &primary),
                ("child-backup", &child_backup),
                ("parent-backup", &parent_backup),
            ],
            json!({
                "primary":{"kind":"load_balancer","upstream":"primary"},
                "child-backup":{"kind":"load_balancer","upstream":"child-backup"},
                "parent-backup":{"kind":"load_balancer","upstream":"parent-backup"},
                "child":{"kind":"failover","service":"primary","failovers":["child-backup"],"on_status":[404]},
                "root":{"kind":"failover","service":"child","failovers":["parent-backup"],"on_status":[503]}
            }),
        );
        assert_eq!(
            execute(&runtime, ctx::DispatchKind::Primary)
                .await
                .status()
                .as_u16(),
            expected
        );
        assert_eq!(primary.records().len(), 1);
        assert_eq!(child_backup.records().len(), child_calls);
        assert_eq!(parent_backup.records().len(), parent_calls);
    }
}

#[tokio::test]
async fn each_entered_mirror_boundary_dispatches_once_and_unused_fallback_mirrors_stay_idle() {
    for status in [200, 503] {
        let primary = Upstream::start(Outcome::Status(status)).await;
        let backup = Upstream::start(Outcome::Status(200)).await;
        let root_shadow = Upstream::start(Outcome::Status(200)).await;
        let first_shadow = Upstream::start(Outcome::Status(200)).await;
        let backup_shadow = Upstream::start(Outcome::Status(200)).await;
        let nested_shadow = Upstream::start(Outcome::Status(200)).await;
        let runtime = runtime_for(
            &[
                ("primary", &primary),
                ("backup", &backup),
                ("root-shadow", &root_shadow),
                ("first-shadow", &first_shadow),
                ("backup-shadow", &backup_shadow),
                ("nested-shadow", &nested_shadow),
            ],
            json!({
                "primary":{"kind":"load_balancer","upstream":"primary"},
                "backup":{"kind":"load_balancer","upstream":"backup"},
                "root-shadow":{"kind":"load_balancer","upstream":"root-shadow"},
                "first-shadow":{"kind":"load_balancer","upstream":"first-shadow"},
                "backup-shadow-leaf":{"kind":"load_balancer","upstream":"backup-shadow"},
                "nested-shadow":{"kind":"load_balancer","upstream":"nested-shadow"},
                "backup-shadow":{"kind":"mirror","service":"backup-shadow-leaf","mirrors":[{"service":"nested-shadow","percent":100}]},
                "first":{"kind":"mirror","service":"primary","mirrors":[{"service":"first-shadow","percent":100}]},
                "fallback":{"kind":"mirror","service":"backup","mirrors":[{"service":"backup-shadow","percent":100}]},
                "failover":{"kind":"failover","service":"first","failovers":["fallback"],"on_status":[503]},
                "root":{"kind":"mirror","service":"failover","mirrors":[{"service":"root-shadow","percent":100}]}
            }),
        );
        let plan = build_execution_plan(&runtime.core.graph, "root", "request-id").unwrap();
        let response = execute_plan_with_request(
            &runtime,
            &plan,
            Request::new(Body::empty()),
            &state(&runtime),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), 200);
        drop(response);
        wait_for_mirrors(&runtime).await;
        assert_eq!(primary.records().len(), 1);
        assert_eq!(backup.records().len(), usize::from(status == 503));
        assert_eq!(root_shadow.records().len(), 1);
        assert_eq!(first_shadow.records().len(), 1);
        assert_eq!(backup_shadow.records().len(), usize::from(status == 503));
        assert_eq!(nested_shadow.records().len(), usize::from(status == 503));
        assert_eq!(runtime.resources.available().2, 32);
    }
}

#[tokio::test]
async fn unused_fallback_mirrors_do_not_buffer_or_reserve_capacity_for_mutations() {
    for status in [200, 503] {
        let primary = Upstream::start(Outcome::Status(status)).await;
        let backup = Upstream::start(Outcome::Status(200)).await;
        let shadow = Upstream::start(Outcome::Status(200)).await;
        let runtime = runtime_for(
            &[
                ("primary", &primary),
                ("backup", &backup),
                ("shadow", &shadow),
            ],
            json!({
                "primary":{"kind":"load_balancer","upstream":"primary"},
                "backup":{"kind":"load_balancer","upstream":"backup"},
                "shadow":{"kind":"load_balancer","upstream":"shadow"},
                "fallback":{"kind":"mirror","service":"backup","mirrors":[{"service":"shadow","percent":100}]},
                "root":{"kind":"failover","service":"primary","failovers":["fallback"],"on_status":[503]}
            }),
        );
        let memory = runtime.resources.reserve_replay(32).unwrap();
        let plan = build_execution_plan(&runtime.core.graph, "root", "request-id").unwrap();
        let request = Request::builder()
            .method("POST")
            .body(Body::from("larger than the replay cap"))
            .unwrap();
        let response = execute_plan_with_request(&runtime, &plan, request, &state(&runtime))
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        assert_eq!(primary.records()[0].body, "larger than the replay cap");
        assert!(backup.records().is_empty());
        assert!(shadow.records().is_empty());
        assert_eq!(
            runtime.resources.available().1,
            runtime.settings.budgets.mirror_concurrency
        );
        drop(memory);
        drop(response);
        make_unavailable(&runtime, "primary", &primary);
        let request = Request::builder()
            .method("POST")
            .body(Body::from("body"))
            .unwrap();
        let response = execute_plan_with_request(&runtime, &plan, request, &state(&runtime))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        drop(response);
        wait_for_mirrors(&runtime).await;
        assert_eq!(primary.records().len(), 1);
        assert_eq!(backup.records().len(), 1);
        assert_eq!(shadow.records().len(), 1);
        assert_eq!(backup.records()[0].method, Method::POST);
        assert_eq!(shadow.records()[0].body, "body");
        assert_eq!(runtime.resources.available().2, 32);
    }
}

#[tokio::test]
async fn unavailable_mirror_main_does_not_transfer_its_shadows_to_the_next_branch() {
    let primary = Upstream::start(Outcome::Status(200)).await;
    let backup = Upstream::start(Outcome::Status(200)).await;
    let shadow = Upstream::start(Outcome::Status(200)).await;
    let runtime = runtime_for(
        &[
            ("primary", &primary),
            ("backup", &backup),
            ("shadow", &shadow),
        ],
        json!({
            "primary":{"kind":"load_balancer","upstream":"primary"},
            "backup":{"kind":"load_balancer","upstream":"backup"},
            "shadow":{"kind":"load_balancer","upstream":"shadow"},
            "first":{"kind":"mirror","service":"primary","mirrors":[{"service":"shadow","percent":100}]},
            "root":{"kind":"failover","service":"first","failovers":["backup"]}
        }),
    );
    make_unavailable(&runtime, "primary", &primary);
    let plan = build_execution_plan(&runtime.core.graph, "root", "request-id").unwrap();
    let request = Request::builder()
        .method("POST")
        .body(Body::from("larger than replay cap"))
        .unwrap();
    let response = execute_plan_with_request(&runtime, &plan, request, &state(&runtime))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(primary.records().is_empty());
    assert_eq!(backup.records().len(), 1);
    assert!(shadow.records().is_empty());
    assert_eq!(runtime.resources.available().2, 32);
}

#[tokio::test]
async fn unused_fallback_does_not_reserve_a_half_open_circuit_probe() {
    let primary = Upstream::start(Outcome::Status(200)).await;
    let backup = Upstream::start(Outcome::Status(200)).await;
    let mut runtime = runtime_for(
        &[("primary", &primary), ("backup", &backup)],
        json!({
            "primary":{"kind":"load_balancer","upstream":"primary"},
            "backup":{"kind":"load_balancer","upstream":"backup"},
            "root":{"kind":"failover","service":"primary","failovers":["backup"],"on_status":[503]}
        }),
    );
    let upstream = lb::Upstream::with_circuit_breaker(backup.url.clone(), None, 1, 0);
    let circuit = upstream.circuit_breaker.clone();
    circuit.record_failure();
    Arc::get_mut(&mut runtime).unwrap().core.balancers.insert(
        "backup".into(),
        lb::BaseLoadBalancer::new(lb::RoundRobin::new(), vec![upstream]),
    );
    let plan = build_execution_plan(&runtime.core.graph, "root", "request-id").unwrap();
    assert_eq!(circuit.state(), 1); // Open; cooldown elapsed, probe unreserved.
    assert_eq!(
        execute_plan_from_replay(
            &runtime,
            &plan,
            &replay(Method::GET),
            &state(&runtime),
            ctx::DispatchKind::Primary
        )
        .await
        .unwrap()
        .status(),
        200
    );
    assert_eq!(circuit.state(), 1);
    assert!(backup.records().is_empty());

    make_unavailable(&runtime, "primary", &primary);
    assert_eq!(
        execute_plan_from_replay(
            &runtime,
            &plan,
            &replay(Method::GET),
            &state(&runtime),
            ctx::DispatchKind::Shadow
        )
        .await
        .unwrap()
        .status(),
        200
    );
    assert_eq!(backup.records().len(), 1);
    assert_eq!(circuit.state(), 2); // Entered shadow reserved the probe; no health feedback.
}

#[tokio::test]
async fn unavailable_remaining_branches_preserve_the_last_response_or_transport_error() {
    for outcome in [Outcome::Status(503), Outcome::CommitThenClose] {
        let primary = Upstream::start(outcome).await;
        let backup = Upstream::start(Outcome::Status(200)).await;
        let runtime = runtime_for(
            &[("primary", &primary), ("backup", &backup)],
            json!({
                "primary":{"kind":"load_balancer","upstream":"primary"},
                "backup":{"kind":"load_balancer","upstream":"backup"},
                "inner":{"kind":"failover","service":"backup","failovers":["backup"],"on_status":[404]},
                "root":{"kind":"failover","service":"primary","failovers":["inner"],"on_status":[503]}
            }),
        );
        make_unavailable(&runtime, "backup", &backup);
        let plan = build_execution_plan(&runtime.core.graph, "root", "request-id").unwrap();
        let result = execute_plan_from_replay(
            &runtime,
            &plan,
            &replay(Method::GET),
            &state(&runtime),
            ctx::DispatchKind::Primary,
        )
        .await;
        match outcome {
            Outcome::Status(_) => {
                let response = result.unwrap();
                assert_eq!(response.status(), 503);
                assert_eq!(
                    response.into_body().collect().await.unwrap().to_bytes(),
                    "upstream response"
                );
            }
            Outcome::CommitThenClose => assert_eq!(
                result.unwrap_err().code,
                ErrorCode::UpstreamConnectionFailed
            ),
        }
        assert_eq!(primary.records().len(), 1);
        assert!(backup.records().is_empty());
    }
}

#[tokio::test]
async fn weighted_nested_transport_failover_completes_before_parent_status_failover() {
    let failing = Upstream::start(Outcome::CommitThenClose).await;
    let child_backup = Upstream::start(Outcome::Status(503)).await;
    let parent_backup = Upstream::start(Outcome::Status(201)).await;
    let unused = Upstream::start(Outcome::Status(200)).await;
    let runtime = runtime_for(
        &[
            ("failing", &failing),
            ("child-backup", &child_backup),
            ("parent-backup", &parent_backup),
            ("unused", &unused),
        ],
        json!({
            "failing":{"kind":"load_balancer","upstream":"failing"},
            "child-backup":{"kind":"load_balancer","upstream":"child-backup"},
            "parent-backup":{"kind":"load_balancer","upstream":"parent-backup"},
            "unused":{"kind":"load_balancer","upstream":"unused"},
            "child":{"kind":"failover","service":"failing","failovers":["child-backup"]},
            "weighted":{"kind":"weighted","services":[{"name":"child","weight":1},{"name":"unused","weight":1}]},
            "local":{"kind":"direct_response","status":503},
            "root":{"kind":"failover","service":"local","failovers":["weighted","parent-backup"],"on_status":[503]}
        }),
    );
    let plan = (0..64)
        .find_map(|seed| {
            let plan =
                build_execution_plan(&runtime.core.graph, "root", &seed.to_string()).unwrap();
            let ExecutionPlan::Failover { services, .. } = &plan else {
                unreachable!()
            };
            matches!(&services[1], ExecutionPlan::Failover { .. }).then_some(plan)
        })
        .unwrap();
    let request = Request::builder()
        .method("PUT")
        .body(Body::from("body"))
        .unwrap();
    assert_eq!(
        execute_plan_with_request(&runtime, &plan, request, &state(&runtime))
            .await
            .unwrap()
            .status(),
        201
    );
    assert_eq!(failing.records().len(), 1);
    assert_eq!(child_backup.records().len(), 1);
    assert_eq!(parent_backup.records().len(), 1);
    for upstream in [&failing, &child_backup, &parent_backup] {
        assert_eq!(upstream.records()[0].method, Method::PUT);
        assert_eq!(upstream.records()[0].body, "body");
    }
    assert!(unused.records().is_empty());
}

#[tokio::test]
async fn unavailable_weighted_choice_skips_to_the_parent_fallback_without_selecting_other_choices()
{
    let unavailable = Upstream::start(Outcome::Status(200)).await;
    let unused = Upstream::start(Outcome::Status(200)).await;
    let backup = Upstream::start(Outcome::Status(201)).await;
    let runtime = runtime_for(
        &[
            ("unavailable", &unavailable),
            ("unused", &unused),
            ("backup", &backup),
        ],
        json!({
            "unavailable":{"kind":"load_balancer","upstream":"unavailable"},
            "unused":{"kind":"load_balancer","upstream":"unused"},
            "backup":{"kind":"load_balancer","upstream":"backup"},
            "weighted":{"kind":"weighted","services":[{"name":"unavailable","weight":1},{"name":"unused","weight":1}]},
            "root":{"kind":"failover","service":"weighted","failovers":["backup"]}
        }),
    );
    make_unavailable(&runtime, "unavailable", &unavailable);
    let plan = (0..64).find_map(|seed| {
        let plan = build_execution_plan(&runtime.core.graph, "root", &seed.to_string()).unwrap();
        let ExecutionPlan::Failover {services, ..} = &plan else { unreachable!() };
        matches!(&services[0], ExecutionPlan::Upstream {service_name, ..} if service_name == "unavailable").then_some(plan)
    }).unwrap();
    let request = Request::builder()
        .method("POST")
        .body(Body::from("larger than the replay cap"))
        .unwrap();
    assert_eq!(
        execute_plan_with_request(&runtime, &plan, request, &state(&runtime))
            .await
            .unwrap()
            .status(),
        201
    );
    assert!(unavailable.records().is_empty());
    assert!(unused.records().is_empty());
    assert_eq!(backup.records().len(), 1);
    assert_eq!(runtime.resources.available().2, 32);
}

#[tokio::test]
async fn local_failover_responses_do_not_require_replay_storage() {
    let runtime = runtime_for(
        &[],
        json!({
            "first":{"kind":"direct_response","status":503},
            "last":{"kind":"direct_response","status":204},
            "root":{"kind":"failover","service":"first","failovers":["last"],"on_status":[503]}
        }),
    );
    let memory = runtime.resources.reserve_replay(32).unwrap();
    let plan = build_execution_plan(&runtime.core.graph, "root", "request-id").unwrap();
    let request = Request::new(Body::from("larger than the replay cap"));
    assert_eq!(
        execute_plan_with_request(&runtime, &plan, request, &state(&runtime))
            .await
            .unwrap()
            .status(),
        204
    );
    drop(memory);
    assert_eq!(runtime.resources.available().2, 32);
}
