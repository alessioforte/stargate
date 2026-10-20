use super::jwt_keyring::{self, JWKS_DIR};
use jwt::{Algorithm, JwtConfig, KeyMaterial};
use once_cell::sync::Lazy;
use std::{env, fs, path::Path};
use tools::parse_duration;
use tracing::info;

const DEFAULT_ACCESS_EXP: &str = "1h";
const DEFAULT_REFRESH_EXP: &str = "1d";
pub(crate) const DEFAULT_KEY_ID: &str = "stargate-current";

pub static JWT_CONFIG: Lazy<JwtConfig> = Lazy::new(|| {
    // Unset means "whatever the active key is"; RS256 for a first start.
    let configured = env::var("JWT_ALGORITHM")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value
                .trim()
                .parse::<Algorithm>()
                .expect("Invalid JWT algorithm")
        });

    let jwt_access_exp =
        env::var("JWT_ACCESS_EXP").unwrap_or_else(|_| DEFAULT_ACCESS_EXP.to_owned());
    let jwt_refresh_exp =
        env::var("JWT_REFRESH_EXP").unwrap_or_else(|_| DEFAULT_REFRESH_EXP.to_owned());
    let access_exp =
        parse_duration(&jwt_access_exp).expect("Invalid JWT access expiration duration");
    let refresh_exp =
        parse_duration(&jwt_refresh_exp).expect("Invalid JWT refresh expiration duration");

    let key_id = env::var("JWT_KID")
        .ok()
        .filter(|kid| !kid.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_KEY_ID.to_owned());
    // Optional RS256 key for ID tokens while JWT_KID is an EC key or an HMAC
    // secret (OpenID Connect requires RS256 support).
    let id_token_key_id = env::var("JWT_ID_TOKEN_KID")
        .ok()
        .map(|kid| kid.trim().to_owned())
        .filter(|kid| !kid.is_empty());

    let invalid = |error: String| -> ! { panic!("Invalid JWT signing keys: {error}") };
    let root = Path::new(JWKS_DIR);
    let (active, additional) = match configured {
        Some(algorithm @ (Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512)) => {
            let secret = env::var("JWT_SECRET").unwrap_or_else(|_| load_or_generate_secret());
            let active = KeyMaterial::from_secret(&key_id, algorithm, secret.as_bytes())
                .unwrap_or_else(|error| invalid(error.to_string()));
            let additional = match id_token_key_id.as_deref() {
                Some(id_key) => jwt_keyring::load_for_id_tokens(root, id_key)
                    .unwrap_or_else(|error| invalid(error)),
                None => Vec::new(),
            };
            (active, additional)
        }
        Some(algorithm) if !jwt_keyring::is_asymmetric(algorithm) => {
            panic!("Unsupported JWT algorithm: {algorithm:?}")
        }
        _ => {
            let keyring = jwt_keyring::load(root, &key_id, id_token_key_id.as_deref(), configured)
                .unwrap_or_else(|error| invalid(error));
            (keyring.active, keyring.additional)
        }
    };

    info!("JWT signing key: {} ({:?})", key_id, active.algorithm());
    for key in &additional {
        if Some(key.key_id()) != id_token_key_id.as_deref() {
            info!(
                "JWT verification key: {} ({:?})",
                key.key_id(),
                key.algorithm()
            );
        }
    }

    let mut config = JwtConfig::from_keyring(active, additional, access_exp, refresh_exp)
        .unwrap_or_else(|error| invalid(error.to_string()));
    if let Some(id_key) = id_token_key_id.as_deref() {
        config = config
            .with_id_token_key(id_key)
            .unwrap_or_else(|error| invalid(error.to_string()));
        if config.id_token_algorithm() != jwt_keyring::ID_TOKEN_ALGORITHM {
            invalid(format!(
                "JWT_ID_TOKEN_KID '{id_key}' is {:?}; ID token keys must be {:?}",
                config.id_token_algorithm(),
                jwt_keyring::ID_TOKEN_ALGORITHM
            ));
        }
        info!(
            "JWT ID token key: {} ({:?})",
            id_key,
            config.id_token_algorithm()
        );
    }
    if config.id_token_algorithm() != Algorithm::RS256 {
        tracing::warn!(
            "ID tokens are signed with {:?}; OpenID Connect requires RS256 support, so certified relying parties may reject this provider. Set JWT_ID_TOKEN_KID to an RS256 key.",
            config.id_token_algorithm()
        );
    }
    config
});

fn load_or_generate_secret() -> String {
    let secret_path = format!("{JWKS_DIR}/secret.key");

    fs::read_to_string(&secret_path).unwrap_or_else(|_| {
        fs::create_dir_all(JWKS_DIR).expect("Unable to create JWKS directory");
        let secret = pw::Generator::new(512).lowercase().digits().generate();
        fs::write(secret_path, &secret).expect("Unable to write secret key");
        secret
    })
}

pub fn init() {
    Lazy::force(&JWT_CONFIG);
}

pub fn jwt_config() -> &'static JwtConfig {
    &JWT_CONFIG
}
