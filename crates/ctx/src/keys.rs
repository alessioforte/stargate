use std::collections::HashMap;
use std::fmt;

use aws_lc_rs::error::KeyRejected;
use aws_lc_rs::rsa::{KeyPair, PublicKey, PublicKeyComponents};
use aws_lc_rs::signature::KeyPair as _;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::DecodingKey;
use jsonwebtoken::jwk::{AlgorithmParameters, Jwk, KeyAlgorithm, KeyOperations, PublicKeyUse};

use crate::error::{KeyMaterialError, KeyResolutionError};
use crate::strict_json;

const MIN_RSA_BITS: usize = 2_048;

/// An RS256-compatible public verification key.
#[derive(Clone)]
pub struct VerificationKey {
    pub(crate) decoding_key: DecodingKey,
}

impl VerificationKey {
    /// Parses an RSA public key from PKCS#8/SPKI or PKCS#1 PEM and enforces the
    /// version 1 minimum key strength.
    pub fn from_rsa_pem(pem: &[u8]) -> Result<Self, KeyMaterialError> {
        let block = pem::parse(pem).map_err(|_| KeyMaterialError::InvalidRsa)?;
        if !matches!(block.tag(), "PUBLIC KEY" | "RSA PUBLIC KEY") {
            return Err(KeyMaterialError::InvalidRsa);
        }
        // aws-lc-rs reads both SubjectPublicKeyInfo and PKCS#1 `RSAPublicKey`.
        let key = PublicKey::from_der(block.contents()).map_err(rejected)?;
        require_rsa_strength(modulus_bits(
            key.modulus().big_endian_without_leading_zero(),
        ))?;
        let decoding_key =
            DecodingKey::from_rsa_pem(pem).map_err(|_| KeyMaterialError::InvalidRsa)?;
        Ok(Self { decoding_key })
    }

    /// Parses one RSA JWK. Network access and JWKS caching intentionally remain
    /// outside the core crate.
    pub fn from_jwk_json(json: &[u8]) -> Result<Self, KeyMaterialError> {
        let value = strict_json::parse(json).map_err(|_| KeyMaterialError::IncompatibleJwk)?;
        let jwk: Jwk =
            serde_json::from_value(value).map_err(|_| KeyMaterialError::IncompatibleJwk)?;
        validate_jwk(&jwk)?;
        let decoding_key =
            DecodingKey::from_jwk(&jwk).map_err(|_| KeyMaterialError::IncompatibleJwk)?;
        Ok(Self { decoding_key })
    }
}

impl fmt::Debug for VerificationKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerificationKey([redacted])")
    }
}

/// Resolves a trusted verification key by protected-header `kid`.
pub trait KeyResolver: Send + Sync {
    fn resolve(&self, key_id: &str) -> Result<VerificationKey, KeyResolutionError>;
}

/// In-memory resolver for tests, local consumers, and preloaded key sets.
#[derive(Default)]
pub struct StaticKeyResolver {
    keys: HashMap<String, VerificationKey>,
}

impl StaticKeyResolver {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an already validated key. An existing key with the same id is
    /// replaced explicitly.
    pub fn insert(&mut self, key_id: impl Into<String>, key: VerificationKey) {
        self.keys.insert(key_id.into(), key);
    }

    pub fn from_rsa_pem(key_id: impl Into<String>, pem: &[u8]) -> Result<Self, KeyMaterialError> {
        let mut resolver = Self::new();
        resolver.insert(key_id, VerificationKey::from_rsa_pem(pem)?);
        Ok(resolver)
    }

    pub fn from_jwk_json(key_id: impl Into<String>, json: &[u8]) -> Result<Self, KeyMaterialError> {
        let key_id = key_id.into();
        let value = strict_json::parse(json).map_err(|_| KeyMaterialError::IncompatibleJwk)?;
        let jwk: Jwk =
            serde_json::from_value(value.clone()).map_err(|_| KeyMaterialError::IncompatibleJwk)?;
        if jwk
            .common
            .key_id
            .as_deref()
            .is_some_and(|jwk_id| jwk_id != key_id)
        {
            return Err(KeyMaterialError::IncompatibleJwk);
        }
        let key = VerificationKey::from_jwk_json(
            &serde_json::to_vec(&value).map_err(|_| KeyMaterialError::IncompatibleJwk)?,
        )?;
        let mut resolver = Self::new();
        resolver.insert(key_id, key);
        Ok(resolver)
    }
}

impl KeyResolver for StaticKeyResolver {
    fn resolve(&self, key_id: &str) -> Result<VerificationKey, KeyResolutionError> {
        self.keys
            .get(key_id)
            .cloned()
            .ok_or(KeyResolutionError::UnknownKey)
    }
}

/// The public half of an RSA key as a JWK carries it: big-endian `n` and `e`
/// without leading zero octets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RsaPublicComponents {
    modulus: Vec<u8>,
    exponent: Vec<u8>,
}

impl RsaPublicComponents {
    /// Reads the public components of an RSA private key in PKCS#8
    /// (`PRIVATE KEY`) or PKCS#1 (`RSA PRIVATE KEY`) PEM.
    pub fn from_private_key_pem(pem: &[u8]) -> Result<Self, KeyMaterialError> {
        Ok(Self::of(&parse_rsa_private_key(pem)?))
    }

    #[must_use]
    pub fn modulus(&self) -> &[u8] {
        &self.modulus
    }

    #[must_use]
    pub fn exponent(&self) -> &[u8] {
        &self.exponent
    }

    fn of(key: &KeyPair) -> Self {
        let PublicKeyComponents { n, e } = PublicKeyComponents::<Vec<u8>>::from(key.public_key());
        Self {
            modulus: n,
            exponent: e,
        }
    }
}

/// A validated RS256 signing key with its public components.
pub(crate) struct SigningKey {
    pub(crate) encoding_key: jsonwebtoken::EncodingKey,
    pub(crate) public_components: RsaPublicComponents,
}

pub(crate) fn parse_private_key(pem: &[u8]) -> Result<SigningKey, KeyMaterialError> {
    let key = parse_rsa_private_key(pem)?;
    let encoding_key =
        jsonwebtoken::EncodingKey::from_rsa_pem(pem).map_err(|_| KeyMaterialError::InvalidRsa)?;
    Ok(SigningKey {
        encoding_key,
        public_components: RsaPublicComponents::of(&key),
    })
}

fn parse_rsa_private_key(pem: &[u8]) -> Result<KeyPair, KeyMaterialError> {
    let block = pem::parse(pem).map_err(|_| KeyMaterialError::InvalidRsa)?;
    match block.tag() {
        "PRIVATE KEY" => KeyPair::from_pkcs8(block.contents()),
        "RSA PRIVATE KEY" => KeyPair::from_der(block.contents()),
        _ => return Err(KeyMaterialError::InvalidRsa),
    }
    .map_err(rejected)
}

/// aws-lc-rs accepts 2048- to 8192-bit RSA keys and names a smaller one
/// `TooSmall`.
fn rejected(error: KeyRejected) -> KeyMaterialError {
    match error.description_() {
        "TooSmall" => KeyMaterialError::RsaTooSmall,
        _ => KeyMaterialError::InvalidRsa,
    }
}

fn validate_jwk(jwk: &Jwk) -> Result<(), KeyMaterialError> {
    if jwk
        .common
        .public_key_use
        .as_ref()
        .is_some_and(|usage| usage != &PublicKeyUse::Signature)
    {
        return Err(KeyMaterialError::IncompatibleJwk);
    }
    if jwk
        .common
        .key_operations
        .as_ref()
        .is_some_and(|operations| {
            operations.is_empty()
                || operations
                    .iter()
                    .any(|operation| operation != &KeyOperations::Verify)
        })
    {
        return Err(KeyMaterialError::IncompatibleJwk);
    }
    if jwk
        .common
        .key_algorithm
        .is_some_and(|algorithm| algorithm != KeyAlgorithm::RS256)
    {
        return Err(KeyMaterialError::IncompatibleJwk);
    }

    let AlgorithmParameters::RSA(parameters) = &jwk.algorithm else {
        return Err(KeyMaterialError::NotRsa);
    };
    let modulus = URL_SAFE_NO_PAD
        .decode(&parameters.n)
        .map_err(|_| KeyMaterialError::IncompatibleJwk)?;
    require_rsa_strength(modulus_bits(&modulus))
}

fn modulus_bits(modulus: &[u8]) -> usize {
    let Some((index, first)) = modulus.iter().enumerate().find(|(_, byte)| **byte != 0) else {
        return 0;
    };
    (modulus.len() - index - 1) * 8 + (8 - first.leading_zeros() as usize)
}

fn require_rsa_strength(bits: usize) -> Result<(), KeyMaterialError> {
    if bits < MIN_RSA_BITS {
        return Err(KeyMaterialError::RsaTooSmall);
    }
    Ok(())
}
