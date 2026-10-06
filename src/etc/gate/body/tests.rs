use gate::cfg::RuntimeConfig;
use std::sync::Arc;

#[tokio::test]
async fn response_body_retains_its_generation_until_consumed_or_dropped() {
    use axum::body::Body;
    use http::Request;
    use http_body_util::BodyExt;

    let config = RuntimeConfig::from_yaml_str(
        r#"
schema: stargate/v1
ingress:
  limit: ingress
  timeout: 250ms
limits:
  default:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 1s
  ingress:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 100ms
http:
  services:
    local:
      kind: direct_response
      status: 200
      body:
        text: ready
  routers:
    local:
      match:
        path:
          prefix: /
      service: local
"#,
    )
    .unwrap()
    .raw;
    for consume in [true, false] {
        let gate = Arc::new(crate::etc::gate::test_support::gate(config.clone()));
        let mut req = Request::builder().uri("/").body(Body::empty()).unwrap();
        req.extensions_mut().insert(gate.clone());
        let body = crate::api::gateway::service(req).await.unwrap().into_body();
        let retained = Arc::downgrade(&gate.snapshot());
        gate.activate(crate::etc::gate::prepare_config(gate::cfg::Config::default()).unwrap())
            .unwrap();
        assert!(retained.upgrade().is_some());
        if consume {
            assert_eq!(body.collect().await.unwrap().to_bytes(), "ready");
        } else {
            drop(body);
        }
        assert!(retained.upgrade().is_none());
    }
}
