use rand::Rng;
use sha2::{Digest, Sha256};

mod meta;

pub fn generate_api_key() -> String {
    generate_api_key_with(meta::KeyType::Secret, meta::KeyEnvironment::Live)
}

pub fn generate_admin_key() -> String {
    generate_api_key_with(meta::KeyType::Admin, meta::KeyEnvironment::Live)
}

pub fn generate_api_key_with(key_type: meta::KeyType, env: meta::KeyEnvironment) -> String {
    // 32 random bytes (256 bits), base64url-encoded to 43 URL-safe chars.
    let mut secret = [0u8; 32];
    rand::rng().fill_bytes(&mut secret);

    format!(
        "{}_{}_{}",
        key_type.as_str(),
        env.as_str(),
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, secret)
    )
}

pub fn hash_api_key(api_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}
