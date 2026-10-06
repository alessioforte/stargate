use super::InternalContextError;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ctx::{MAX_KEY_ID_BYTES, VerificationKey};
use rsa::{BigUint, RsaPrivateKey, pkcs1::DecodeRsaPrivateKey, pkcs8::DecodePrivateKey};
use serde_json::{Map, Value};
use std::collections::HashSet;

pub(super) fn parse_private_key(
    private_key_pem: &[u8],
) -> Result<RsaPrivateKey, InternalContextError> {
    let text = std::str::from_utf8(private_key_pem).map_err(|_| {
        InternalContextError::Signer(ctx::ConfigError::Key(ctx::KeyMaterialError::InvalidRsa))
    })?;
    RsaPrivateKey::from_pkcs8_pem(text)
        .or_else(|_| RsaPrivateKey::from_pkcs1_pem(text))
        .map_err(|_| {
            InternalContextError::Signer(ctx::ConfigError::Key(ctx::KeyMaterialError::InvalidRsa))
        })
}

pub(super) fn validate_and_sanitize_jwks(
    input: &[u8],
    active_key_id: &str,
    private_modulus: &BigUint,
    private_exponent: &BigUint,
) -> Result<Value, InternalContextError> {
    let value: Value =
        serde_json::from_slice(input).map_err(|_| InternalContextError::InvalidJwksShape)?;
    let keys = value
        .as_object()
        .and_then(|object| object.get("keys"))
        .and_then(Value::as_array)
        .filter(|keys| !keys.is_empty())
        .ok_or(InternalContextError::InvalidJwksShape)?;

    let mut seen = HashSet::new();
    let mut active_matches = None;
    let mut sanitized = Vec::with_capacity(keys.len());
    for (index, key) in keys.iter().enumerate() {
        let object = key
            .as_object()
            .ok_or(InternalContextError::InvalidJwk { index })?;
        reject_private_members(object, index)?;
        let key_id = object
            .get("kid")
            .and_then(Value::as_str)
            .ok_or(InternalContextError::InvalidKeyId)?;
        validate_key_id(key_id)?;
        if !seen.insert(key_id.to_owned()) {
            return Err(InternalContextError::DuplicateKeyId(key_id.to_owned()));
        }
        if object.get("alg").and_then(Value::as_str) != Some("RS256") {
            return Err(InternalContextError::InvalidJwk { index });
        }

        let serialized =
            serde_json::to_vec(key).map_err(|_| InternalContextError::InvalidJwk { index })?;
        VerificationKey::from_jwk_json(&serialized)
            .map_err(|_| InternalContextError::InvalidJwk { index })?;
        let (modulus, exponent) = jwk_public_parts(object, index)?;
        if key_id == active_key_id {
            active_matches = Some(modulus == *private_modulus && exponent == *private_exponent);
        }

        sanitized.push(Value::Object(sanitize_public_jwk(object)));
    }

    match active_matches {
        None => Err(InternalContextError::MissingActiveKey(
            active_key_id.to_owned(),
        )),
        Some(false) => Err(InternalContextError::KeyPairMismatch),
        Some(true) => Ok(serde_json::json!({ "keys": sanitized })),
    }
}

fn reject_private_members(
    object: &Map<String, Value>,
    index: usize,
) -> Result<(), InternalContextError> {
    const PRIVATE_MEMBERS: [&str; 9] = ["d", "p", "q", "dp", "dq", "qi", "oth", "k", "key"];
    if PRIVATE_MEMBERS
        .iter()
        .any(|name| object.contains_key(*name))
    {
        return Err(InternalContextError::PrivateJwkMaterial { index });
    }
    Ok(())
}

fn validate_key_id(key_id: &str) -> Result<(), InternalContextError> {
    if key_id.trim().is_empty()
        || key_id.len() > MAX_KEY_ID_BYTES
        || !key_id.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(InternalContextError::InvalidKeyId);
    }
    Ok(())
}

fn jwk_public_parts(
    object: &Map<String, Value>,
    index: usize,
) -> Result<(BigUint, BigUint), InternalContextError> {
    let decode = |name: &str| {
        object
            .get(name)
            .and_then(Value::as_str)
            .ok_or(InternalContextError::InvalidJwk { index })
            .and_then(|value| {
                URL_SAFE_NO_PAD
                    .decode(value)
                    .map(|bytes| BigUint::from_bytes_be(&bytes))
                    .map_err(|_| InternalContextError::InvalidJwk { index })
            })
    };
    Ok((decode("n")?, decode("e")?))
}

fn sanitize_public_jwk(object: &Map<String, Value>) -> Map<String, Value> {
    ["kty", "use", "key_ops", "alg", "kid", "n", "e"]
        .into_iter()
        .filter_map(|name| {
            object
                .get(name)
                .cloned()
                .map(|value| (name.to_owned(), value))
        })
        .collect()
}
