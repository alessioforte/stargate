use rand_core::OsRng;
use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
use rsa::{RsaPrivateKey, RsaPublicKey};

pub fn generate_rsa_keys(bits: usize) -> Result<(String, String), Box<dyn std::error::Error>> {
    let mut rng = OsRng;
    let private_key = RsaPrivateKey::new(&mut rng, bits)?;
    let public_key = RsaPublicKey::from(&private_key);
    let private_pem = private_key.to_pkcs8_pem(LineEnding::LF)?;
    let public_pem = public_key.to_public_key_pem(LineEnding::LF)?;
    Ok((private_pem.to_string(), public_pem.to_string()))
}
