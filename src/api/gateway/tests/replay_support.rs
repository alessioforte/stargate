use crate::api::gateway::{
    execution::{plan::ExecutionPlan, replay::ReplayRequest},
    lifecycle::Execution,
    request::RequestState,
};
use crate::etc::gate::RuntimeSnapshot;
use axum::{body::Body, response::Response};
use gate::cfg::Config;
use http::{HeaderMap, Method, Request};
use http_body_util::BodyExt;
use hyper::{body::Bytes, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use std::{
    io,
    sync::{Arc, Mutex},
};
use tokio::{
    net::TcpListener,
    task::{JoinHandle, JoinSet},
};

#[derive(Clone, Copy)]
pub(super) enum Outcome {
    CommitThenClose,
    Status(u16),
}

#[derive(Clone)]
pub(super) struct CommittedRequest {
    pub(super) method: Method,
    pub(super) headers: HeaderMap,
    pub(super) body: Bytes,
}

pub(super) struct Upstream {
    pub(super) url: String,
    committed: Arc<Mutex<Vec<CommittedRequest>>>,
    task: JoinHandle<()>,
}

impl Upstream {
    pub(super) async fn start(outcome: Outcome) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let committed = Arc::new(Mutex::new(Vec::new()));
        let records = committed.clone();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (socket, _) = accepted.unwrap();
                        let records = records.clone();
                        connections.spawn(async move {
                            let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                                let records = records.clone();
                                async move {
                                    let (parts, body) = request.into_parts();
                                    let body = body.collect().await.map_err(io::Error::other)?.to_bytes();
                                    records.lock().unwrap().push(CommittedRequest { method: parts.method, headers: parts.headers, body });
                                    match outcome {
                                        // The operation has committed. Fail the connection before
                                        // any response headers can tell the gateway it succeeded.
                                        Outcome::CommitThenClose => Err(io::Error::other("closed after commit")),
                                        Outcome::Status(status) => Ok(Response::builder().status(status).body(Body::from("upstream response")).unwrap()),
                                    }
                                }
                            });
                            let _ = http1::Builder::new().serve_connection(TokioIo::new(socket), service).await;
                        });
                    }
                    _ = connections.join_next(), if !connections.is_empty() => {}
                }
            }
        });
        Self {
            url,
            committed,
            task,
        }
    }

    pub(super) fn records(&self) -> Vec<CommittedRequest> {
        self.committed.lock().unwrap().clone()
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) fn config(primary: &Upstream, backup: &Upstream) -> Config {
    let mut value = serde_json::to_value(Config::default()).unwrap();
    value["runtime"] =
        serde_json::json!({"replay_body_bytes":8,"replay_memory_bytes":32,"request_timeout":"2s"});
    value["limits"]["default"] =
        serde_json::json!({"strategy":"gcra","params":{"max_burst":100,"replenish_1_per":"1h"}});
    value["http"] = serde_json::json!({
        "upstreams": {
            "primary":{"targets":[{"url":primary.url}],"load_balancer":{"strategy":"round_robin","circuit_breaker":{"fail_threshold":1,"cooldown":"1h"}}},
            "backup":{"targets":[{"url":backup.url}]}
        },
        "services": {
            "primary":{"kind":"load_balancer","upstream":"primary"},
            "backup":{"kind":"load_balancer","upstream":"backup"},
            "entry":{"kind":"failover","service":"primary","failovers":["backup"],"on_status":[503]}
        },
        "routers":{"entry":{"match":{"path":{"prefix":"/"}},"service":"entry"}}
    });
    serde_json::from_value(value).unwrap()
}

pub(super) fn state(runtime: &RuntimeSnapshot) -> RequestState {
    RequestState {
        execution: Execution::new(
            runtime.settings.request_timeout,
            runtime.resources.shutdown.child_token(),
        ),
        client_ip: "127.0.0.1".into(),
        load_balancer_key: None,
        original_path: "/orders".into(),
        path: "/orders".into(),
        query: String::new(),
        preserve_host: false,
        response_headers: Default::default(),
        propagation_draft: None,
        internal_context_runtime: None,
    }
}

pub(super) fn planned(name: &str) -> ExecutionPlan {
    ExecutionPlan::Upstream {
        service_name: name.into(),
        internal_context: None,
    }
}

pub(super) fn plan() -> ExecutionPlan {
    ExecutionPlan::Failover {
        services: vec![planned("primary"), planned("backup")],
        on_status: vec![503],
    }
}

pub(super) fn replay(method: Method) -> ReplayRequest {
    let mut headers = HeaderMap::new();
    headers.insert("idempotency-key", "the-same-key".parse().unwrap());
    ReplayRequest {
        method,
        version: http::Version::HTTP_11,
        headers,
        body: Bytes::from_static(b"body"),
    }
}
