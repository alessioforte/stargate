mod claims;
mod config;
mod jwk_set;
mod keyring;
mod keys;

pub use claims::Claims;
pub use claims::issuer_from_env;
pub use config::{JwtConfig, JwtValidationError};
pub use jsonwebtoken::Algorithm;
pub use jsonwebtoken::errors::{Error as JwtError, ErrorKind as JwtErrorKind};
pub use jsonwebtoken::jwk::{AlgorithmParameters, EllipticCurve, Jwk, JwkSet, PublicKeyUse};
pub use jwk_set::{JWK_SET_ALGORITHMS, decode_with_jwk_set, validate_public_jwk_set};
pub use keyring::{KeyError, KeyMaterial, MAX_KEY_ID_BYTES, infer_algorithm, validate_key_id};
pub use keys::{generate_p256_keys, generate_p384_keys, generate_rsa_keys};
