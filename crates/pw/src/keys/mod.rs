use rand::Rng;
use rand::distr::Alphanumeric;
use sha2::{Digest, Sha256};

mod meta;
// mod parse;

pub fn generate_api_key() -> String {
    generate_api_key_with(meta::KeyType::Secret, meta::KeyEnvironment::Live)
}

pub fn generate_api_key_with(key_type: meta::KeyType, env: meta::KeyEnvironment) -> String {
    let key: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(32) // Length of the API key
        .map(char::from)
        .collect();

    format!(
        "{}_{}_{}",
        key_type.as_str(),
        env.as_str(),
        base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            key.as_bytes()
        )
    )
}

pub fn hash_api_key(api_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}
