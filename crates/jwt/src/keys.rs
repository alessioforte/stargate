use p256::elliptic_curve::Generate;
use p256::pkcs8::{
    EncodePrivateKey as _, EncodePublicKey as _, LineEnding as EllipticCurveLineEnding,
};
use rsa::pkcs8::{EncodePrivateKey as _, EncodePublicKey as _, LineEnding as RsaLineEnding};
use rsa::{RsaPrivateKey, RsaPublicKey};

pub fn generate_rsa_keys(bits: usize) -> Result<(String, String), Box<dyn std::error::Error>> {
    let mut rng = rsa::rand_core::OsRng;
    let private_key = RsaPrivateKey::new(&mut rng, bits)?;
    let public_key = RsaPublicKey::from(&private_key);
    let private_pem = private_key.to_pkcs8_pem(RsaLineEnding::LF)?;
    let public_pem = public_key.to_public_key_pem(RsaLineEnding::LF)?;
    Ok((private_pem.to_string(), public_pem.to_string()))
}

pub fn generate_p256_keys() -> Result<(String, String), Box<dyn std::error::Error>> {
    let private_key = p256::SecretKey::generate();
    let public_key = private_key.public_key();
    let private_pem = private_key.to_pkcs8_pem(EllipticCurveLineEnding::LF)?;
    let public_pem = public_key.to_public_key_pem(EllipticCurveLineEnding::LF)?;
    Ok((private_pem.to_string(), public_pem))
}

pub fn generate_p384_keys() -> Result<(String, String), Box<dyn std::error::Error>> {
    let private_key = p384::SecretKey::generate();
    let public_key = private_key.public_key();
    let private_pem = private_key.to_pkcs8_pem(EllipticCurveLineEnding::LF)?;
    let public_pem = public_key.to_public_key_pem(EllipticCurveLineEnding::LF)?;
    Ok((private_pem.to_string(), public_pem))
}
