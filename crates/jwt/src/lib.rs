mod claims;
mod config;
mod keyring;
mod keys;

pub use claims::Claims;
pub use claims::issuer_from_env;
pub use config::{JwtConfig, JwtValidationError, KeySource};
pub use jsonwebtoken::Algorithm;
pub use jsonwebtoken::errors::Error as JwtError;
pub use jsonwebtoken::jwk::{AlgorithmParameters, EllipticCurve, Jwk, JwkSet, PublicKeyUse};
pub use keyring::{KeyError, KeyMaterial, MAX_KEY_ID_BYTES, infer_algorithm, validate_key_id};
pub use keys::{generate_p256_keys, generate_p384_keys, generate_rsa_keys};
