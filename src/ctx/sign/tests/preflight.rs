use super::*;

#[test]
fn absent_block_skips_key_preflight() {
    let config = config(
        "schema: stargate/v1\ningress:\n  limit: ingress\n  timeout: 250ms\nlimits:\n  default:\n    strategy: gcra\n    params:\n      max_burst: 100\n      replenish_1_per: 1s\n  ingress:\n    strategy: gcra\n    params:\n      max_burst: 100\n      replenish_1_per: 100ms\nhttp: {}\n",
    );
    preflight_config_with(&config, || panic!("loader must not run")).unwrap();
}

#[test]
fn reload_candidate_failure_occurs_before_activation() {
    let config = config(
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
  upstreams:
    orders:
      targets:
        - url: http://orders:8080
      internal_context:
        audience: urn:stargate:service:orders
"#,
    );
    let error = preflight_config_with(&config, || Err(InternalContextError::NotConfigured))
        .expect_err("candidate must fail preflight");
    assert!(matches!(error, InternalContextError::NotConfigured));
}

#[test]
fn valid_present_block_passes_preflight() {
    let config = config(
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
  upstreams:
    orders:
      targets:
        - url: http://orders:8080
      internal_context:
        audience: urn:stargate:service:orders
"#,
    );
    let jwk: Value = serde_json::from_str(PUBLIC_JWK).unwrap();
    let runtime = InternalContextRuntime::from_material(
        settings("stargate-internal-test"),
        PRIVATE_KEY,
        &jwks(jwk),
    )
    .unwrap();
    preflight_config_with(&config, || Ok(Some(Arc::new(runtime)))).unwrap();
}
