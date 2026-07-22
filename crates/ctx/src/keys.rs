use std::collections::HashMap;
use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::DecodingKey;
use jsonwebtoken::jwk::{AlgorithmParameters, Jwk, KeyAlgorithm, KeyOperations, PublicKeyUse};
use rsa::pkcs1::{DecodeRsaPrivateKey, DecodeRsaPublicKey};
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey};
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, RsaPublicKey};

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
        let text = std::str::from_utf8(pem).map_err(|_| KeyMaterialError::InvalidRsa)?;
        let key = RsaPublicKey::from_public_key_pem(text)
            .or_else(|_| RsaPublicKey::from_pkcs1_pem(text))
            .map_err(|_| KeyMaterialError::InvalidRsa)?;
        require_rsa_strength(key.n().bits())?;
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

pub(crate) fn parse_private_key(pem: &[u8]) -> Result<jsonwebtoken::EncodingKey, KeyMaterialError> {
    let text = std::str::from_utf8(pem).map_err(|_| KeyMaterialError::InvalidRsa)?;
    let key = RsaPrivateKey::from_pkcs8_pem(text)
        .or_else(|_| RsaPrivateKey::from_pkcs1_pem(text))
        .map_err(|_| KeyMaterialError::InvalidRsa)?;
    require_rsa_strength(key.n().bits())?;
    jsonwebtoken::EncodingKey::from_rsa_pem(pem).map_err(|_| KeyMaterialError::InvalidRsa)
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
