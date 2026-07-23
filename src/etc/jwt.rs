use jwt::{Algorithm, JwtConfig, KeySource};
use once_cell::sync::Lazy;
use std::{env, fs, path::Path};
use tools::parse_duration;
use tracing::info;

const JWKS_PATH: &str = ".stargate/jwks";
const DEFAULT_JWT_ALGORITHM: &str = "RS256";
const DEFAULT_ACCESS_EXP: &str = "1h";
const DEFAULT_REFRESH_EXP: &str = "1d";
const DEFAULT_KEY_ID: &str = "stargate-current";
const RSA_KEY_BITS: usize = 2048;

pub static JWT_CONFIG: Lazy<JwtConfig> = Lazy::new(|| {
    let algorithm = env::var("JWT_ALGORITHM")
        .unwrap_or_else(|_| DEFAULT_JWT_ALGORITHM.to_owned())
        .parse::<Algorithm>()
        .expect("Invalid JWT algorithm");

    info!("JWT Algorithm: {:?}", algorithm);

    let key_source = match algorithm {
        Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
            let secret = env::var("JWT_SECRET").unwrap_or_else(|_| load_or_generate_secret());
            KeySource::Secret(secret)
        }
        Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
            let (private_key_path, public_key_path) =
                ensure_key_pair(JWKS_PATH, || jwt::generate_rsa_keys(RSA_KEY_BITS));
            KeySource::Rsa {
                private_key_path,
                public_key_path,
            }
        }
        Algorithm::ES256 => {
            let jwks_path = format!("{JWKS_PATH}/es256");
            let (private_key_path, public_key_path) =
                ensure_key_pair(&jwks_path, jwt::generate_p256_keys);
            KeySource::Ec {
                private_key_path,
                public_key_path,
            }
        }
        Algorithm::ES384 => {
            let jwks_path = format!("{JWKS_PATH}/es384");
            let (private_key_path, public_key_path) =
                ensure_key_pair(&jwks_path, jwt::generate_p384_keys);
            KeySource::Ec {
                private_key_path,
                public_key_path,
            }
        }
        _ => panic!("Unsupported algorithm: {:?}", algorithm),
    };

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

    JwtConfig::new_with_key_id(algorithm, key_source, access_exp, refresh_exp, Some(key_id))
});

fn load_or_generate_secret() -> String {
    let secret_path = format!("{JWKS_PATH}/secret.key");

    fs::read_to_string(&secret_path).unwrap_or_else(|_| {
        fs::create_dir_all(JWKS_PATH).expect("Unable to create JWKS directory");
        let secret = pw::Generator::new(512).lowercase().digits().generate();
        fs::write(secret_path, &secret).expect("Unable to write secret key");
        secret
    })
}

fn ensure_key_pair<F>(jwks_path: &str, generate: F) -> (String, String)
where
    F: FnOnce() -> Result<(String, String), Box<dyn std::error::Error>>,
{
    let private_key_path = format!("{jwks_path}/private.pem");
    let public_key_path = format!("{jwks_path}/public.pem");

    if !Path::new(&private_key_path).exists() || !Path::new(&public_key_path).exists() {
        fs::create_dir_all(jwks_path).expect("Unable to create JWKS directory");
        let (private_key, public_key) = generate().expect("Failed to generate JWT signing keys");
        fs::write(&private_key_path, private_key).expect("Unable to write private key");
        fs::write(&public_key_path, public_key).expect("Unable to write public key");
    }

    (private_key_path, public_key_path)
}

pub fn init() {
    Lazy::force(&JWT_CONFIG);
}

pub fn jwt_config() -> &'static JwtConfig {
    &JWT_CONFIG
}
