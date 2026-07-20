use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ctx::{ContextSigner, MAX_CLOCK_SKEW_SECS, MAX_KEY_ID_BYTES, SignerConfig, VerificationKey};
use gate::cfg::RuntimeConfig;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::traits::PublicKeyParts;
use rsa::{BigUint, RsaPrivateKey};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

const ALGORITHM_ENV: &str = "INTERNAL_CONTEXT_ALGORITHM";
const ISSUER_ENV: &str = "INTERNAL_CONTEXT_ISSUER";
const KEY_ID_ENV: &str = "INTERNAL_CONTEXT_KID";
const PRIVATE_KEY_PATH_ENV: &str = "INTERNAL_CONTEXT_PRIVATE_KEY_PATH";
const JWKS_PATH_ENV: &str = "INTERNAL_CONTEXT_JWKS_PATH";
const TTL_ENV: &str = "INTERNAL_CONTEXT_TTL_SECS";
const CLOCK_SKEW_ENV: &str = "INTERNAL_CONTEXT_CLOCK_SKEW_SECS";
const CACHE_MAX_AGE_ENV: &str = "INTERNAL_CONTEXT_JWKS_CACHE_MAX_AGE_SECS";

const DEFAULT_TTL_SECS: u64 = 30;
const DEFAULT_CLOCK_SKEW_SECS: u64 = 5;
const DEFAULT_CACHE_MAX_AGE_SECS: u64 = 60;

static RUNTIME: OnceLock<Option<Arc<InternalContextRuntime>>> = OnceLock::new();

#[derive(Debug, thiserror::Error)]
pub enum InternalContextError {
    #[error("internal-context runtime has already been initialized")]
    AlreadyInitialized,
    #[error("{0} is required when internal context is configured")]
    MissingSetting(&'static str),
    #[error("{ALGORITHM_ENV} must be RS256 for version 1")]
    UnsupportedAlgorithm,
    #[error("{name} must be an unsigned integer")]
    InvalidInteger { name: &'static str },
    #[error("{CLOCK_SKEW_ENV} must not exceed {MAX_CLOCK_SKEW_SECS} seconds")]
    InvalidClockSkew,
    #[error("unable to read internal-context {kind} file '{path}': {source}")]
    ReadFile {
        kind: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid internal-context signer configuration: {0}")]
    Signer(#[source] ctx::ConfigError),
    #[error("internal-context JWKS must be a JSON object containing a nonempty 'keys' array")]
    InvalidJwksShape,
    #[error("internal-context JWKS key {index} must be a public RS256 RSA JWK")]
    InvalidJwk { index: usize },
    #[error("internal-context JWKS key {index} contains private key material")]
    PrivateJwkMaterial { index: usize },
    #[error(
        "internal-context JWKS key ids must be nonblank, printable ASCII, and at most {MAX_KEY_ID_BYTES} bytes"
    )]
    InvalidKeyId,
    #[error("internal-context JWKS contains duplicate kid '{0}'")]
    DuplicateKeyId(String),
    #[error("internal-context JWKS does not contain the active kid '{0}'")]
    MissingActiveKey(String),
    #[error("the active internal-context public JWK does not match the private signing key")]
    KeyPairMismatch,
    #[error("internal context is not configured")]
    NotConfigured,
    #[error("internal_context activation is guarded until P5; configured upstreams: {upstreams}")]
    ActivationGuard { upstreams: String },
}

#[derive(Debug)]
pub struct InternalContextRuntime {
    _signer: Arc<ContextSigner>,
    public_jwks: Value,
    cache_max_age_secs: u64,
    _clock_skew_secs: u64,
}

impl InternalContextRuntime {
    pub fn public_jwks(&self) -> &Value {
        &self.public_jwks
    }

    pub const fn cache_max_age_secs(&self) -> u64 {
        self.cache_max_age_secs
    }

    fn from_material(
        settings: RuntimeSettings,
        private_key_pem: &[u8],
        jwks_json: &[u8],
    ) -> Result<Self, InternalContextError> {
        if settings.clock_skew_secs > MAX_CLOCK_SKEW_SECS {
            return Err(InternalContextError::InvalidClockSkew);
        }

        let signer_config =
            SignerConfig::new(settings.issuer, settings.key_id.clone(), settings.ttl_secs)
                .map_err(InternalContextError::Signer)?;
        let signer = ContextSigner::from_rsa_pem(signer_config, private_key_pem)
            .map_err(InternalContextError::Signer)?;
        let private_key = parse_private_key(private_key_pem)?;
        let public_jwks = validate_and_sanitize_jwks(
            jwks_json,
            &settings.key_id,
            private_key.n(),
            private_key.e(),
        )?;

        Ok(Self {
            _signer: Arc::new(signer),
            public_jwks,
            cache_max_age_secs: settings.cache_max_age_secs,
            _clock_skew_secs: settings.clock_skew_secs,
        })
    }
}

#[derive(Debug)]
struct RuntimeSettings {
    issuer: String,
    key_id: String,
    private_key_path: PathBuf,
    jwks_path: PathBuf,
    ttl_secs: u64,
    clock_skew_secs: u64,
    cache_max_age_secs: u64,
}

impl RuntimeSettings {
    fn from_lookup(
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Option<Self>, InternalContextError> {
        let names = [
            ALGORITHM_ENV,
            ISSUER_ENV,
            KEY_ID_ENV,
            PRIVATE_KEY_PATH_ENV,
            JWKS_PATH_ENV,
            TTL_ENV,
            CLOCK_SKEW_ENV,
            CACHE_MAX_AGE_ENV,
        ];
        if !names.iter().any(|name| lookup(name).is_some()) {
            return Ok(None);
        }

        let algorithm = lookup(ALGORITHM_ENV).unwrap_or_else(|| "RS256".to_owned());
        if algorithm != "RS256" {
            return Err(InternalContextError::UnsupportedAlgorithm);
        }

        Ok(Some(Self {
            issuer: required(&lookup, ISSUER_ENV)?,
            key_id: required(&lookup, KEY_ID_ENV)?,
            private_key_path: PathBuf::from(required(&lookup, PRIVATE_KEY_PATH_ENV)?),
            jwks_path: PathBuf::from(required(&lookup, JWKS_PATH_ENV)?),
            ttl_secs: unsigned_or_default(&lookup, TTL_ENV, DEFAULT_TTL_SECS)?,
            clock_skew_secs: unsigned_or_default(&lookup, CLOCK_SKEW_ENV, DEFAULT_CLOCK_SKEW_SECS)?,
            cache_max_age_secs: unsigned_or_default(
                &lookup,
                CACHE_MAX_AGE_ENV,
                DEFAULT_CACHE_MAX_AGE_SECS,
            )?,
        }))
    }
}

pub fn init() -> Result<(), InternalContextError> {
    let runtime = load_from_env()?;
    RUNTIME
        .set(runtime)
        .map_err(|_| InternalContextError::AlreadyInitialized)
}

pub fn runtime() -> Option<&'static Arc<InternalContextRuntime>> {
    RUNTIME.get().and_then(Option::as_ref)
}

pub fn preflight_config(config: &RuntimeConfig) -> Result<(), InternalContextError> {
    preflight_config_with(config, load_from_env)
}

fn preflight_config_with(
    config: &RuntimeConfig,
    load: impl FnOnce() -> Result<Option<Arc<InternalContextRuntime>>, InternalContextError>,
) -> Result<(), InternalContextError> {
    let upstreams = config
        .compiled()
        .http
        .upstreams
        .values()
        .filter(|upstream| upstream.internal_context.is_some())
        .map(|upstream| upstream.name.as_str())
        .collect::<Vec<_>>();
    if upstreams.is_empty() {
        return Ok(());
    }

    load()?.ok_or(InternalContextError::NotConfigured)?;
    Err(InternalContextError::ActivationGuard {
        upstreams: upstreams.join(", "),
    })
}

fn load_from_env() -> Result<Option<Arc<InternalContextRuntime>>, InternalContextError> {
    load_with(|name| env::var(name).ok(), |path| std::fs::read(path))
}

fn load_with(
    lookup: impl Fn(&str) -> Option<String>,
    mut read: impl FnMut(&Path) -> Result<Vec<u8>, std::io::Error>,
) -> Result<Option<Arc<InternalContextRuntime>>, InternalContextError> {
    let Some(settings) = RuntimeSettings::from_lookup(lookup)? else {
        return Ok(None);
    };
    let private_key =
        read(&settings.private_key_path).map_err(|source| InternalContextError::ReadFile {
            kind: "private key",
            path: settings.private_key_path.clone(),
            source,
        })?;
    let jwks = read(&settings.jwks_path).map_err(|source| InternalContextError::ReadFile {
        kind: "JWKS",
        path: settings.jwks_path.clone(),
        source,
    })?;
    Ok(Some(Arc::new(InternalContextRuntime::from_material(
        settings,
        &private_key,
        &jwks,
    )?)))
}

fn required(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &'static str,
) -> Result<String, InternalContextError> {
    lookup(name)
        .filter(|value| !value.trim().is_empty())
        .ok_or(InternalContextError::MissingSetting(name))
}

fn unsigned_or_default(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &'static str,
    default: u64,
) -> Result<u64, InternalContextError> {
    lookup(name)
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| InternalContextError::InvalidInteger { name })
        })
        .unwrap_or(Ok(default))
}

fn parse_private_key(private_key_pem: &[u8]) -> Result<RsaPrivateKey, InternalContextError> {
    let text = std::str::from_utf8(private_key_pem).map_err(|_| {
        InternalContextError::Signer(ctx::ConfigError::Key(ctx::KeyMaterialError::InvalidRsa))
    })?;
    RsaPrivateKey::from_pkcs8_pem(text)
        .or_else(|_| RsaPrivateKey::from_pkcs1_pem(text))
        .map_err(|_| {
            InternalContextError::Signer(ctx::ConfigError::Key(ctx::KeyMaterialError::InvalidRsa))
        })
}

fn validate_and_sanitize_jwks(
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

#[cfg(test)]
mod tests {
    use super::*;

    const PRIVATE_KEY: &[u8] = include_bytes!("../../crates/ctx/tests/fixtures/private.pem");
    const PUBLIC_JWK: &str = include_str!("../../crates/ctx/tests/fixtures/public.jwk.json");

    fn settings(key_id: &str) -> RuntimeSettings {
        RuntimeSettings {
            issuer: "https://stargate.test/internal-context".to_owned(),
            key_id: key_id.to_owned(),
            private_key_path: PathBuf::from("private.pem"),
            jwks_path: PathBuf::from("jwks.json"),
            ttl_secs: DEFAULT_TTL_SECS,
            clock_skew_secs: DEFAULT_CLOCK_SKEW_SECS,
            cache_max_age_secs: DEFAULT_CACHE_MAX_AGE_SECS,
        }
    }

    fn jwks(jwk: Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({ "keys": [jwk] })).unwrap()
    }

    fn config(yaml: &str) -> RuntimeConfig {
        RuntimeConfig::from_yaml_str(yaml).expect("config should compile")
    }

    #[test]
    fn no_environment_is_an_absent_runtime() {
        let loaded = RuntimeSettings::from_lookup(|_| None).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn rejects_non_rs256_algorithm() {
        let error = RuntimeSettings::from_lookup(|name| match name {
            ALGORITHM_ENV => Some("ES256".to_owned()),
            ISSUER_ENV | KEY_ID_ENV | PRIVATE_KEY_PATH_ENV | JWKS_PATH_ENV => {
                Some("configured".to_owned())
            }
            _ => None,
        })
        .unwrap_err();
        assert!(matches!(error, InternalContextError::UnsupportedAlgorithm));
    }

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
    fn missing_private_key_fails_loading() {
        let error = load_with(
            |name| match name {
                ISSUER_ENV => Some("https://stargate.test/internal-context".to_owned()),
                KEY_ID_ENV => Some("stargate-internal-test".to_owned()),
                PRIVATE_KEY_PATH_ENV => Some("missing-private.pem".to_owned()),
                JWKS_PATH_ENV => Some("jwks.json".to_owned()),
                _ => None,
            },
            |_| Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
        )
        .unwrap_err();
        assert!(matches!(error, InternalContextError::ReadFile { .. }));
    }

    #[test]
    fn mismatched_active_kid_fails_loading() {
        let jwk: Value = serde_json::from_str(PUBLIC_JWK).unwrap();
        let error = InternalContextRuntime::from_material(
            settings("different-key"),
            PRIVATE_KEY,
            &jwks(jwk),
        )
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

    #[test]
    fn absent_block_skips_key_preflight() {
        let config = config("schema: stargate/v2alpha1\nhttp: {}\n");
        preflight_config_with(&config, || panic!("loader must not run")).unwrap();
    }

    #[test]
    fn reload_candidate_failure_occurs_before_activation() {
        let config = config(
            r#"
schema: stargate/v2alpha1
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
    fn valid_present_block_remains_activation_guarded() {
        let config = config(
            r#"
schema: stargate/v2alpha1
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
        let error = preflight_config_with(&config, || Ok(Some(Arc::new(runtime)))).unwrap_err();
        assert!(matches!(
            error,
            InternalContextError::ActivationGuard { .. }
        ));
    }
}
