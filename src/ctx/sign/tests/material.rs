use super::*;

#[test]
fn loads_matching_private_key_and_public_jwks() {
    let jwk: Value = serde_json::from_str(PUBLIC_JWK).unwrap();
    let runtime = InternalContextRuntime::from_material(
        settings("stargate-internal-test"),
        PRIVATE_KEY,
        &jwks(jwk),
    )
    .unwrap();
    assert_eq!(runtime.public_jwks()["keys"].as_array().unwrap().len(), 1);
    assert_eq!(runtime.cache_max_age_secs(), DEFAULT_CACHE_MAX_AGE_SECS);
}

#[test]
fn mismatched_active_kid_fails_loading() {
    let jwk: Value = serde_json::from_str(PUBLIC_JWK).unwrap();
    let error =
        InternalContextRuntime::from_material(settings("different-key"), PRIVATE_KEY, &jwks(jwk))
            .unwrap_err();
    assert!(matches!(
        error,
        InternalContextError::MissingActiveKey(key) if key == "different-key"
    ));
}

#[test]
fn mismatched_active_key_pair_fails_loading() {
    let mut jwk: Value = serde_json::from_str(PUBLIC_JWK).unwrap();
    let modulus = jwk["n"].as_str().unwrap();
    let mut modulus = URL_SAFE_NO_PAD.decode(modulus).unwrap();
    modulus[10] ^= 1;
    jwk["n"] = Value::String(URL_SAFE_NO_PAD.encode(modulus));
    let error = InternalContextRuntime::from_material(
        settings("stargate-internal-test"),
        PRIVATE_KEY,
        &jwks(jwk),
    )
    .unwrap_err();
    assert!(matches!(error, InternalContextError::KeyPairMismatch));
}

#[test]
fn public_jwks_supports_rotation_overlap() {
    let active: Value = serde_json::from_str(PUBLIC_JWK).unwrap();
    let mut retiring = active.clone();
    retiring["kid"] = Value::String("stargate-internal-retiring".to_owned());
    let input = serde_json::to_vec(&serde_json::json!({
        "keys": [active, retiring]
    }))
    .unwrap();
    let runtime = InternalContextRuntime::from_material(
        settings("stargate-internal-test"),
        PRIVATE_KEY,
        &input,
    )
    .unwrap();
    assert_eq!(runtime.public_jwks()["keys"].as_array().unwrap().len(), 2);
}

#[test]
fn rotation_drill_accepts_both_keys_before_retiring_the_old_key() {
    let current_key_id = "stargate-internal-current";
    let next_key_id = "stargate-internal-next";
    let current_jwk = public_jwk(PRIVATE_KEY, current_key_id);
    let next_jwk = public_jwk(ROTATION_NEXT_PRIVATE_KEY, next_key_id);
    let overlapping_jwks = serde_json::to_vec(&serde_json::json!({
        "keys": [current_jwk.clone(), next_jwk.clone()]
    }))
    .unwrap();

    let current_runtime = InternalContextRuntime::from_material(
        settings(current_key_id),
        PRIVATE_KEY,
        &overlapping_jwks,
    )
    .unwrap();
    let next_runtime = InternalContextRuntime::from_material(
        settings(next_key_id),
        ROTATION_NEXT_PRIVATE_KEY,
        &overlapping_jwks,
    )
    .unwrap();
    let request = anonymous_issue_request();
    let current_token = current_runtime.issue(&request).unwrap();
    let next_token = next_runtime.issue(&request).unwrap();
    let expected = ctx::ExpectedRequest {
        method: "POST",
        encoded_path: "/v1/orders",
        request_id: "01JZ000000000000000000000R",
    };

    let overlap_verifier = verifier(&[current_jwk.clone(), next_jwk.clone()]);
    assert!(
        overlap_verifier
            .verify(current_token.compact(), expected)
            .is_ok()
    );
    assert!(
        overlap_verifier
            .verify(next_token.compact(), expected)
            .is_ok()
    );

    let retired_verifier = verifier(&[next_jwk]);
    assert!(
        retired_verifier
            .verify(next_token.compact(), expected)
            .is_ok()
    );
    assert_eq!(
        retired_verifier.verify(current_token.compact(), expected),
        Err(ctx::VerificationError::KeyResolution)
    );
}

#[test]
fn private_jwk_members_are_rejected() {
    let mut jwk: Value = serde_json::from_str(PUBLIC_JWK).unwrap();
    jwk["d"] = Value::String("private".to_owned());
    let error = InternalContextRuntime::from_material(
        settings("stargate-internal-test"),
        PRIVATE_KEY,
        &jwks(jwk),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        InternalContextError::PrivateJwkMaterial { index: 0 }
    ));
}

#[test]
fn jwks_response_drops_unrecognized_members() {
    let mut jwk: Value = serde_json::from_str(PUBLIC_JWK).unwrap();
    jwk["operator_note"] = Value::String("do not publish".to_owned());
    let runtime = InternalContextRuntime::from_material(
        settings("stargate-internal-test"),
        PRIVATE_KEY,
        &jwks(jwk),
    )
    .unwrap();
    assert!(runtime.public_jwks()["keys"][0]["operator_note"].is_null());
}
