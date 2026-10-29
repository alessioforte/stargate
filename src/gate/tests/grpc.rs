use crate::gate::{Gate, service, test_support};
use axum::{Extension, Router, body::Body};
use futures_util::StreamExt;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use hyper::body::{Bytes, Frame};
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use lb::LoadBalancer;
use std::{sync::Arc, time::Duration};
use tokio::{net::TcpListener, task::JoinHandle};
use tonic_health::{
    ServingStatus,
    pb::{HealthCheckRequest, health_client::HealthClient},
};

struct Server {
    url: String,
    task: JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn health_server() -> (Server, tonic_health::server::HealthReporter) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let incoming = futures_util::stream::unfold(listener, |listener| async move {
        let connection = listener.accept().await.map(|(stream, _)| stream);
        Some((connection, listener))
    });
    let (reporter, health) = tonic_health::server::health_reporter();
    let task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(health)
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });
    (Server { url, task }, reporter)
}

fn config(url: &str) -> gate::cfg::Config {
    serde_json::from_value(serde_json::json!({
        "schema":"stargate/v1",
        "ingress":{"limit":"ingress","timeout":"250ms"},
        "limits":{
            "default":{"strategy":"gcra","params":{"max_burst":1000,"replenish_1_per":"1ms"}},
            "ingress":{"strategy":"gcra","params":{"max_burst":1000,"replenish_1_per":"1ms"}}
        },
        "http":{
            "upstreams":{"up":{"targets":[{"url":url}],"transport":{"protocols":["http2"]}}},
            "services":{"up":{"kind":"load_balancer","upstream":"up"}},
            "routers":{"up":{"match":{"path":{"prefix":"/"}},"service":"up","response_mode":"stream"}}
        }
    })).unwrap()
}

async fn gateway(gate: Arc<Gate>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .fallback_service(tower::service_fn(service))
        .layer(Extension(gate));
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Server { url, task }
}

async fn http_server(router: Router) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Server { url, task }
}

fn http2_client() -> Client<HttpConnector, Body> {
    Client::builder(TokioExecutor::new())
        .http2_only(true)
        .build_http()
}

#[tokio::test]
async fn tonic_unary_and_server_streaming_pass_through_gateway() {
    let (upstream, reporter) = health_server().await;
    let gate = Arc::new(test_support::gate(config(&upstream.url)));
    let proxy = gateway(gate).await;
    let channel = tonic::transport::Endpoint::from_shared(proxy.url.clone())
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut client = HealthClient::new(channel);
    let response = client
        .check(HealthCheckRequest { service: "".into() })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(response.status, 1);
    let mut stream = client
        .watch(HealthCheckRequest { service: "".into() })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(stream.message().await.unwrap().unwrap().status, 1);
    reporter
        .set_service_status("", ServingStatus::NotServing)
        .await;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), stream.message())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .status,
        2
    );
    reporter
        .set_service_status("", ServingStatus::Serving)
        .await;
    for protocols in [serde_json::json!(["http1", "http2"]), serde_json::json!([])] {
        let mut raw = serde_json::to_value(config(&upstream.url)).unwrap();
        raw["http"]["upstreams"]["up"]["transport"]["protocols"] = protocols;
        let another = gateway(Arc::new(test_support::gate(
            serde_json::from_value(raw).unwrap(),
        )))
        .await;
        let channel = tonic::transport::Endpoint::from_shared(another.url.clone())
            .unwrap()
            .connect()
            .await
            .unwrap();
        let mut client = HealthClient::new(channel);
        assert_eq!(
            client
                .check(HealthCheckRequest { service: "".into() })
                .await
                .unwrap()
                .into_inner()
                .status,
            1
        );
    }
}

#[tokio::test]
async fn grpc_probe_tracks_serving_and_not_serving() {
    let (upstream, reporter) = health_server().await;
    reporter
        .set_service_status("example.Service", ServingStatus::Serving)
        .await;
    let target = lb::Upstream::with_circuit_breaker(
        upstream.url.clone(),
        Some(lb::HealthProbe::Grpc {
            service: "example.Service".into(),
        }),
        1,
        3600,
    );
    let balancer = lb::BaseLoadBalancer::new(lb::RoundRobin::new(), vec![target]);
    balancer.set_health_client(
        reqwest::Client::builder()
            .http2_prior_knowledge()
            .build()
            .unwrap(),
    );
    let context = lb::RequestContext {
        client_ip: "127.0.0.1",
        path: "/",
        method: "POST",
        key: None,
    };
    balancer.health_check().await;
    assert!(balancer.select(&context).is_some());
    reporter
        .set_service_status("example.Service", ServingStatus::NotServing)
        .await;
    balancer.health_check().await;
    assert!(balancer.select(&context).is_none());
}

#[tokio::test]
async fn grpc_probe_uses_prepared_upstream_mtls() {
    crate::srv::tls::install_crypto_provider();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "https://localhost:{}",
        listener.local_addr().unwrap().port()
    );
    let address = listener.local_addr().unwrap();
    let certificates = rustls_pemfile::certs(&mut &test_support::SERVER_CERT[..])
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key = rustls_pemfile::private_key(&mut &test_support::SERVER_KEY[..])
        .unwrap()
        .unwrap();
    let ca = rustls_pemfile::certs(&mut &test_support::CA[..])
        .next()
        .unwrap()
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca).unwrap();
    let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots.clone()))
        .build()
        .unwrap();
    let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_client_cert_verifier(verifier)
    .with_single_cert(certificates, key)
    .unwrap();
    tls.alpn_protocols = vec![b"h2".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
    let rejected = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let incoming = futures_util::stream::unfold(
        (listener, acceptor, rejected.clone()),
        |(listener, acceptor, rejected)| async move {
            loop {
                let stream = match listener.accept().await {
                    Ok((stream, _)) => stream,
                    Err(error) => return Some((Err(error), (listener, acceptor, rejected))),
                };
                match acceptor.accept(stream).await {
                    Ok(stream) if stream.get_ref().1.alpn_protocol() == Some(b"h2") => {
                        return Some((Ok(stream), (listener, acceptor, rejected)));
                    }
                    Ok(_) => {
                        rejected.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    Err(_) => {}
                }
            }
        },
    );
    let (_, health) = tonic_health::server::health_reporter();
    let task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(health)
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });
    let _server = Server {
        url: url.clone(),
        task,
    };
    let client_cert = rustls_pemfile::certs(&mut &test_support::CLIENT_CERT[..])
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let client_key = rustls_pemfile::private_key(&mut &test_support::CLIENT_KEY[..])
        .unwrap()
        .unwrap();
    let client_tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_client_auth_cert(client_cert, client_key)
    .unwrap();
    let _no_alpn = tokio_rustls::TlsConnector::from(Arc::new(client_tls))
        .connect(
            "localhost".try_into().unwrap(),
            tokio::net::TcpStream::connect(address).await.unwrap(),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while rejected.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let files = test_support::TlsFiles::new();
    let mut raw = serde_json::to_value(config(&url)).unwrap();
    raw["mtls"] = serde_json::to_value(files.mtls()).unwrap();
    raw["http"]["upstreams"]["up"]["load_balancer"] = serde_json::json!({
        "strategy":"round_robin", "liveness_probe":{"protocol":"grpc", "interval":"1h"},
        "circuit_breaker":{"fail_threshold":1,"cooldown":"1h"}
    });
    let gate = test_support::gate(serde_json::from_value(raw).unwrap());
    let snapshot = gate.snapshot();
    let balancer = &snapshot.core.balancers["up"];
    balancer.health_check().await;
    let context = lb::RequestContext {
        client_ip: "127.0.0.1",
        path: "/",
        method: "POST",
        key: None,
    };
    assert!(balancer.select(&context).is_some());
}

#[tokio::test]
async fn grpc_rejects_http1_only_upstream_without_contacting_it() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tracing_subscriber::{Layer, layer::SubscriberExt};
    struct ErrorCounter(Arc<AtomicUsize>);
    impl<S: tracing::Subscriber> Layer<S> for ErrorCounter {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _: tracing_subscriber::layer::Context<'_, S>,
        ) {
            struct CodeVisitor(bool);
            impl tracing::field::Visit for CodeVisitor {
                fn record_debug(
                    &mut self,
                    field: &tracing::field::Field,
                    value: &dyn std::fmt::Debug,
                ) {
                    if field.name() == "code"
                        && format!("{value:?}") == "gateway.grpc_http2_required"
                    {
                        self.0 = true;
                    }
                }
            }
            if *event.metadata().level() == tracing::Level::ERROR {
                let mut visitor = CodeVisitor(false);
                event.record(&mut visitor);
                if visitor.0 {
                    self.0.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
    }
    let errors = Arc::new(AtomicUsize::new(0));
    let subscriber = tracing_subscriber::registry().with(ErrorCounter(errors.clone()));
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let upstream = http_server(Router::new().fallback(move || {
        let observed = observed.clone();
        async move {
            observed.fetch_add(1, Ordering::SeqCst);
            StatusCode::OK
        }
    }))
    .await;
    let mut raw = serde_json::to_value(config(&upstream.url)).unwrap();
    raw["http"]["upstreams"]["up"]["transport"]["protocols"] = serde_json::json!(["http1"]);
    let gate = Arc::new(test_support::gate(serde_json::from_value(raw).unwrap()));
    let proxy = gateway(gate).await;
    let client = http2_client();
    for _ in 0..2 {
        let response = client
            .request(
                Request::post(format!("{}/call", proxy.url))
                    .header("content-type", "application/grpc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["grpc-status"], "14");
        assert_eq!(
            response.headers()["grpc-message"],
            "Upstream does not allow HTTP/2, which gRPC requires"
        );
        assert!(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .is_empty()
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(errors.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn grpc_http1_protocol_failover_reaches_http2_and_records_metric() {
    use opentelemetry_sdk::{
        error::OTelSdkResult,
        metrics::{
            PeriodicReader, SdkMeterProvider, Temporality,
            data::{AggregatedMetrics, MetricData, ResourceMetrics},
            exporter::PushMetricExporter,
        },
    };
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    // The gateway's instruments use a global provider, so isolate this test in a
    // fresh process without disturbing metrics used by other tests.
    const CHILD: &str = "STARGATE_TEST_GRPC_PROTOCOL_METRIC_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "gate::tests::grpc::grpc_http1_protocol_failover_reaches_http2_and_records_metric",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    #[derive(Clone)]
    struct Exporter(Arc<Mutex<Vec<(String, u64)>>>);
    impl PushMetricExporter for Exporter {
        async fn export(&self, metrics: &ResourceMetrics) -> OTelSdkResult {
            for metric in metrics.scope_metrics().flat_map(|scope| scope.metrics()) {
                if metric.name() == "stargate.gateway.failovers"
                    && let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data()
                {
                    for point in sum.data_points() {
                        if let Some(reason) = point
                            .attributes()
                            .find(|attribute| attribute.key.as_str() == "stargate.reason")
                        {
                            self.0
                                .lock()
                                .unwrap()
                                .push((reason.value.to_string(), point.value()));
                        }
                    }
                }
            }
            Ok(())
        }
        fn force_flush(&self) -> OTelSdkResult {
            Ok(())
        }
        fn shutdown_with_timeout(&self, _: Duration) -> OTelSdkResult {
            Ok(())
        }
        fn temporality(&self) -> Temporality {
            Temporality::Cumulative
        }
    }
    let readings = Arc::new(Mutex::new(Vec::new()));
    let reader = PeriodicReader::builder(Exporter(readings.clone()))
        .with_interval(Duration::from_secs(3600))
        .build();
    let provider = SdkMeterProvider::builder().with_reader(reader).build();
    opentelemetry::global::set_meter_provider(provider.clone());

    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let primary = http_server(Router::new().fallback(move || {
        let observed = observed.clone();
        async move {
            observed.fetch_add(1, Ordering::SeqCst);
            StatusCode::OK
        }
    }))
    .await;
    let backup = http_server(Router::new().route(
        "/call",
        axum::routing::get(|| async { (StatusCode::OK, [("grpc-status", "0")]) }),
    ))
    .await;
    let mut raw = serde_json::to_value(config(&primary.url)).unwrap();
    raw["http"]["upstreams"]["up"]["transport"]["protocols"] = serde_json::json!(["http1"]);
    raw["http"]["upstreams"]["backup"] =
        serde_json::json!({"targets":[{"url":backup.url}],"transport":{"protocols":["http2"]}});
    raw["http"]["services"]["backup"] =
        serde_json::json!({"kind":"load_balancer","upstream":"backup"});
    raw["http"]["services"]["failover"] =
        serde_json::json!({"kind":"failover","service":"up","failovers":["backup"]});
    raw["http"]["routers"]["up"]["service"] = serde_json::json!("failover");
    let proxy = gateway(Arc::new(test_support::gate(
        serde_json::from_value(raw).unwrap(),
    )))
    .await;
    let response = http2_client()
        .request(
            Request::get(format!("{}/call", proxy.url))
                .header("content-type", "application/grpc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["grpc-status"], "0");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    provider.force_flush().unwrap();
    assert_eq!(&*readings.lock().unwrap(), &[("protocol".to_owned(), 1)]);
}

#[tokio::test]
async fn grpc_timeout_is_reduced_for_each_failover_attempt() {
    use axum::routing::get;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let first_sender = sender.clone();
    let primary = http_server(Router::new().route(
        "/call",
        get(move |request: Request<Body>| {
            let sender = first_sender.clone();
            async move {
                sender
                    .send(
                        crate::gate::grpc::timeout(request.headers())
                            .unwrap()
                            .unwrap(),
                    )
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(80)).await;
                StatusCode::SERVICE_UNAVAILABLE
            }
        }),
    ))
    .await;
    let backup = http_server(Router::new().route(
        "/call",
        get(move |request: Request<Body>| {
            let sender = sender.clone();
            async move {
                sender
                    .send(
                        crate::gate::grpc::timeout(request.headers())
                            .unwrap()
                            .unwrap(),
                    )
                    .unwrap();
                (StatusCode::OK, [("grpc-status", "0")])
            }
        }),
    ))
    .await;
    let mut raw = serde_json::to_value(config(&primary.url)).unwrap();
    raw["http"]["upstreams"]["backup"] =
        serde_json::json!({"targets":[{"url":backup.url}],"transport":{"protocols":["http2"]}});
    raw["http"]["services"]["backup"] =
        serde_json::json!({"kind":"load_balancer","upstream":"backup"});
    raw["http"]["services"]["failover"] = serde_json::json!({"kind":"failover","service":"up","failovers":["backup"],"on_status":[503]});
    raw["http"]["routers"]["up"]["service"] = serde_json::json!("failover");
    raw["http"]["routers"]["up"]["timeouts"] = serde_json::json!({"request":"500ms"});
    let proxy = gateway(Arc::new(test_support::gate(
        serde_json::from_value(raw).unwrap(),
    )))
    .await;
    let response = http2_client()
        .request(
            Request::get(format!("{}/call", proxy.url))
                .header("content-type", "application/grpc")
                .header("grpc-timeout", "2S")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["grpc-status"], "0");
    let first = receiver.recv().await.unwrap();
    let second = receiver.recv().await.unwrap();
    assert!(first <= Duration::from_millis(500));
    assert!(second <= Duration::from_millis(500));
    assert!(first.saturating_sub(second) >= Duration::from_millis(40));
}

#[tokio::test]
async fn grpc_transport_is_pinned_before_header_middleware() {
    use axum::routing::post;
    let upstream = http_server(Router::new().route(
        "/call",
        post(|request: Request<Body>| async move {
            assert_eq!(request.version(), http::Version::HTTP_2);
            assert_eq!(request.headers()["content-type"], "application/json");
            (
                [("content-type", "application/grpc"), ("grpc-status", "0")],
                Body::empty(),
            )
        }),
    ))
    .await;
    let mut raw = serde_json::to_value(config(&upstream.url)).unwrap();
    raw["http"]["upstreams"]["up"]["transport"]["protocols"] =
        serde_json::json!(["http1", "http2"]);
    raw["http"]["middlewares"]["replace"] = serde_json::json!({"kind":"request_headers", "set":[{"name":"content-type", "value":"application/json"}]});
    raw["http"]["routers"]["up"]["middlewares"] = serde_json::json!(["replace"]);
    let proxy = gateway(Arc::new(test_support::gate(
        serde_json::from_value(raw).unwrap(),
    )))
    .await;
    let request = Request::builder()
        .method("POST")
        .uri(format!("{}/call", proxy.url))
        .header("content-type", "application/grpc")
        .body(Body::empty())
        .unwrap();
    let response = http2_client().request(request).await.unwrap();
    assert_eq!(response.headers()["grpc-status"], "0");
}

#[tokio::test]
async fn grpc_timeout_applies_before_and_after_response_headers() {
    use axum::routing::post;
    let router = Router::new()
        .route(
            "/slow",
            post(|| async {
                tokio::time::sleep(Duration::from_millis(300)).await;
                (
                    [("content-type", "application/grpc"), ("grpc-status", "0")],
                    Body::empty(),
                )
            }),
        )
        .route(
            "/stream",
            post(|| async {
                let body = Body::from_stream(futures_util::stream::pending::<
                    Result<Bytes, std::convert::Infallible>,
                >());
                ([("content-type", "application/grpc")], body)
            }),
        );
    let upstream = http_server(router).await;
    let gate = Arc::new(test_support::gate(config(&upstream.url)));
    let initial_proxy = gateway(gate).await;
    let client = http2_client();
    for path in ["/slow", "/stream"] {
        let request = Request::builder()
            .method("POST")
            .uri(format!("{}{path}", initial_proxy.url))
            .header("content-type", "application/grpc")
            .header("grpc-timeout", "100m")
            .body(Body::empty())
            .unwrap();
        let started = tokio::time::Instant::now();
        let response = client.request(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        if path == "/slow" {
            assert_eq!(response.headers()["grpc-status"], "4");
        } else {
            let collected = response.into_body().collect().await.unwrap();
            assert_eq!(collected.trailers().unwrap()["grpc-status"], "4");
        }
        assert!(started.elapsed() < Duration::from_millis(700));
    }

    let mut raw = serde_json::to_value(config(&upstream.url)).unwrap();
    raw["runtime"]["request_timeout"] = "100ms".into();
    raw["http"]["routers"]["up"]["timeouts"] = serde_json::json!({"request":"1s"});
    let proxy = gateway(Arc::new(test_support::gate(
        serde_json::from_value(raw).unwrap(),
    )))
    .await;
    let request = Request::builder()
        .method("POST")
        .uri(format!("{}/slow", proxy.url))
        .header("content-type", "application/grpc")
        .body(Body::empty())
        .unwrap();
    let response = client.request(request).await.unwrap();
    assert_eq!(response.headers()["grpc-status"], "0");
}

#[tokio::test]
async fn client_and_bidirectional_streams_allow_long_upload_pauses() {
    use axum::routing::post;
    let upstream = http_server(Router::new().route(
        "/echo",
        post(|request: Request<Body>| async move {
            let frames = futures_util::stream::unfold(
                (request.into_body(), false),
                |(mut body, sent_trailers)| async move {
                    if sent_trailers {
                        return None;
                    }
                    match body.frame().await {
                        Some(Ok(frame)) => {
                            Some((Ok::<_, std::convert::Infallible>(frame), (body, false)))
                        }
                        Some(Err(_)) => None,
                        None => {
                            let mut trailers = http::HeaderMap::new();
                            trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
                            Some((Ok(Frame::trailers(trailers)), (body, true)))
                        }
                    }
                },
            );
            (
                [("content-type", "application/grpc")],
                Body::new(http_body_util::StreamBody::new(frames)),
            )
        }),
    ))
    .await;
    let mut raw = serde_json::to_value(config(&upstream.url)).unwrap();
    raw["runtime"]["request_timeout"] = "2s".into();
    raw["runtime"]["upload_idle_timeout"] = "100ms".into();
    raw["http"]["routers"]["up"]["timeouts"] =
        serde_json::json!({"request":"none", "upload_idle":"none"});
    let gate = Arc::new(test_support::gate(serde_json::from_value(raw).unwrap()));
    let gateway = gateway(gate.clone()).await;
    let frames = futures_util::stream::unfold(0, |index| async move {
        match index {
            0 => Some((
                Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from_static(
                    b"\x00\x00\x00\x00\x05first",
                ))),
                1,
            )),
            1 => {
                tokio::time::sleep(Duration::from_millis(300)).await;
                Some((
                    Ok(Frame::data(Bytes::from_static(
                        b"\x00\x00\x00\x00\x06second",
                    ))),
                    2,
                ))
            }
            _ => None,
        }
    });
    let request = Request::builder()
        .method("POST")
        .uri(format!("{}/echo", gateway.url))
        .header("content-type", "application/grpc")
        .body(Body::new(http_body_util::StreamBody::new(frames)))
        .unwrap();
    let client = http2_client();
    let response = client.request(request).await.unwrap();
    let collected = tokio::time::timeout(Duration::from_secs(3), response.into_body().collect())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(collected.trailers().unwrap()["grpc-status"], "0");
    assert_eq!(
        collected.to_bytes(),
        Bytes::from_static(b"\x00\x00\x00\x00\x05first\x00\x00\x00\x00\x06second")
    );
    assert_eq!(
        gate.resources.available().0,
        gate.resources.budgets.primary_concurrency
    );
}

#[tokio::test]
async fn cancelled_grpc_client_releases_gateway_admission() {
    use axum::routing::post;
    let upstream = http_server(Router::new().route(
        "/stream",
        post(|| async {
            let frames = futures_util::stream::once(async {
                Ok::<_, std::convert::Infallible>(Bytes::from_static(b"\x00\x00\x00\x00\x01x"))
            })
            .chain(futures_util::stream::pending());
            (
                [("content-type", "application/grpc")],
                Body::from_stream(frames),
            )
        }),
    ))
    .await;
    let gate = Arc::new(test_support::gate(config(&upstream.url)));
    let gateway = gateway(gate.clone()).await;
    let client = http2_client();
    let request = Request::builder()
        .method("POST")
        .uri(format!("{}/stream", gateway.url))
        .header("content-type", "application/grpc")
        .body(Body::empty())
        .unwrap();
    let mut response = client.request(request).await.unwrap();
    assert!(response.body_mut().frame().await.unwrap().is_ok());
    drop(response);
    tokio::time::timeout(Duration::from_secs(2), async {
        while gate.resources.available().0 != gate.resources.budgets.primary_concurrency {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn grpc_auth_rejections_and_timeout_validation_use_grpc_status() {
    let mut raw = serde_json::to_value(config("http://127.0.0.1:1")).unwrap();
    raw["http"]["policies"]["auth"] = serde_json::json!({"kind":"auth","strategies":["jwt"]});
    raw["http"]["routers"]["up"]["policies"] = serde_json::json!(["auth"]);
    let gate = Arc::new(test_support::gate(serde_json::from_value(raw).unwrap()));
    for (timeout, expected) in [(None, "16"), (Some("bad"), "3")] {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/demo.Service/Call")
            .header("content-type", "application/grpc");
        if let Some(timeout) = timeout {
            builder = builder.header("grpc-timeout", timeout);
        }
        let mut request = builder.body(Body::empty()).unwrap();
        request.extensions_mut().insert(gate.clone());
        let response = service(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["grpc-status"], expected);
        assert!(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn grpc_access_denial_and_rate_limit_preserve_status_and_headers() {
    use crate::gate::authentication::TestAuthenticator;
    use crate::iam::auth::{
        identity::VerifiedIdentity,
        subject::{Subject, SubjectType},
    };

    let mut raw = serde_json::to_value(config("http://127.0.0.1:1")).unwrap();
    raw["http"]["policies"]["acl"] =
        serde_json::json!({"kind":"access_control","resource":"private","env":"none"});
    raw["http"]["routers"]["up"]["policies"] = serde_json::json!(["acl"]);
    let gate = Arc::new(test_support::gate(serde_json::from_value(raw).unwrap()));
    let mut request = Request::builder()
        .method("POST")
        .uri("/demo.Service/Call")
        .header("content-type", "application/grpc")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(gate);
    request.extensions_mut().insert(TestAuthenticator {
        verify: Arc::new(|_| {
            VerifiedIdentity::test_api_key(Subject::new("test".into(), SubjectType::ApiKey, None))
        }),
    });
    let response = service(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["grpc-status"], "7");

    let mut raw = serde_json::to_value(config("http://127.0.0.1:1")).unwrap();
    raw["limits"]["ingress"]["params"]["max_burst"] = 1.into();
    raw["limits"]["ingress"]["params"]["replenish_1_per"] = "1h".into();
    let gate = Arc::new(test_support::gate(serde_json::from_value(raw).unwrap()));
    for index in 0..2 {
        let mut request = Request::builder()
            .method("POST")
            .uri("/demo.Service/Call")
            .header("content-type", "application/grpc")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(gate.clone());
        let response = service(request).await.unwrap();
        if index == 1 {
            assert_eq!(response.headers()["grpc-status"], "8");
            assert!(response.headers().contains_key("retry-after"));
            assert_eq!(response.headers()["x-ratelimit-scope"], "ingress");
        }
    }
}
