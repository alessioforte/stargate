use aws_lc_rs::encoding::AsDer as _;
use aws_lc_rs::rsa::{KeyPair, KeySize};
use aws_lc_rs::signature::KeyPair as _;
use p256::elliptic_curve::Generate;
use p256::pkcs8::{
    EncodePrivateKey as _, EncodePublicKey as _, LineEnding as EllipticCurveLineEnding,
};

/// RSA modulus sizes accepted by [`generate_rsa_keys`].
pub const RSA_KEY_SIZES: [usize; 4] = [2048, 3072, 4096, 8192];

/// Generates an RSA key pair as PKCS#8 private and SubjectPublicKeyInfo public
/// PEM. `bits` must be one of [`RSA_KEY_SIZES`].
pub fn generate_rsa_keys(bits: usize) -> Result<(String, String), Box<dyn std::error::Error>> {
    let size = rsa_key_size(bits)
        .ok_or_else(|| format!("unsupported RSA key size {bits}; use 2048, 3072, 4096, or 8192"))?;
    let key_pair = KeyPair::generate(size)?;
    let private_der = key_pair.as_der()?;
    let public_der = key_pair.public_key().as_der()?;
    Ok((
        pem_text("PRIVATE KEY", private_der.as_ref()),
        pem_text("PUBLIC KEY", public_der.as_ref()),
    ))
}

fn rsa_key_size(bits: usize) -> Option<KeySize> {
    match bits {
        2048 => Some(KeySize::Rsa2048),
        3072 => Some(KeySize::Rsa3072),
        4096 => Some(KeySize::Rsa4096),
        8192 => Some(KeySize::Rsa8192),
        _ => None,
    }
}

fn pem_text(tag: &str, der: &[u8]) -> String {
    pem::encode_config(
        &pem::Pem::new(tag, der),
        pem::EncodeConfig::new().set_line_ending(pem::LineEnding::LF),
    )
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
