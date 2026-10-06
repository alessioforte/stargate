use crate::api::gateway::routing::router_matches;
use axum::{body::Body, extract::ConnectInfo};
use gate::cfg::Config;
use http::Request;
use serde_json::{Value, json};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

pub(super) fn measure() -> Vec<Value> {
    let mut results = Vec::new();
    for kind in [
        "exact", "prefix", "template", "regex", "host", "header", "query", "cookie", "cidr",
    ] {
        for count in [1, 100, 1000] {
            let mut value = serde_json::to_value(Config::default()).unwrap();
            value["http"]["services"]["local"] = json!({"kind":"direct_response", "status":200});
            for index in 0..count {
                let word = if index == count - 1 {
                    "match".to_string()
                } else {
                    format!("miss{index}")
                };
                let matcher = match kind {
                    "exact" => json!({"path":{"exact":format!("/{word}/42")}}),
                    "prefix" => json!({"path":{"prefix":format!("/{word}")}}),
                    "template" => json!({"path":{"template":format!("/{word}/{{id}}")}}),
                    "regex" => json!({"path":{"regex":format!("^/{word}/[0-9]+$")}}),
                    "host" => json!({"host":{"suffix":format!("{word}.test")}}),
                    "header" => json!({"header":{"name":"x-match", "eq":word}}),
                    "query" => json!({"query":{"name":"kind", "eq":word}}),
                    "cookie" => json!({"cookie":{"name":"kind", "eq":word}}),
                    "cidr" => {
                        json!({"source_ip":{"cidrs":[if index == count-1 {"127.0.0.0/8"} else {"10.0.0.0/8"}]}})
                    }
                    _ => unreachable!(),
                };
                value["http"]["routers"][format!("route{index}")] =
                    json!({"match":matcher, "service":"local"});
            }
            let config: Config = serde_json::from_value(value).unwrap();
            let graph = config.compile().unwrap().http;
            let mut request = Request::builder()
                .uri("/match/42?kind=match")
                .header("host", "api.match.test")
                .header("x-match", "match")
                .header("cookie", "kind=match")
                .body(Body::empty())
                .unwrap();
            request.extensions_mut().insert(ConnectInfo(
                "127.0.0.1:1234".parse::<std::net::SocketAddr>().unwrap(),
            ));
            assert_eq!(
                graph
                    .routers
                    .iter()
                    .position(|router| router_matches(router, &request)),
                Some(count - 1)
            );
            let started = Instant::now();
            let mut samples = Vec::new();
            while started.elapsed() < Duration::from_millis(100) {
                let batch = Instant::now();
                for _ in 0..100 {
                    black_box(
                        graph
                            .routers
                            .iter()
                            .find(|router| router_matches(black_box(router), black_box(&request)))
                            .unwrap(),
                    );
                }
                samples.push(batch.elapsed().as_secs_f64() * 1_000_000.0 / 100.0);
            }
            let result = json!({"matcher":kind, "routers":count, "scan_us":super::reporting::latencies(samples)});
            println!("GATEWAY_ROUTES {result}");
            results.push(result);
        }
    }
    results
}
