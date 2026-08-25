use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use clap::{Args, Subcommand};
use rsa::RsaPrivateKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::traits::PublicKeyParts;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const DEFAULT_OUTPUT_DIR: &str = ".stargate/internal-context";
const DEFAULT_KEY_ID: &str = "stargate-internal-current";
const DEFAULT_RSA_BITS: usize = 2048;
const MIN_RSA_BITS: usize = 2048;
const MAX_RSA_BITS: usize = 8192;

#[derive(Debug, Args)]
pub struct InternalContextArgs {
    #[command(subcommand)]
    command: InternalContextCommand,
}

#[derive(Debug, Subcommand)]
enum InternalContextCommand {
    /// Generate a new private RSA key and matching public JWKS.
    GenerateKey(GenerateKeyArgs),
}

#[derive(Debug, Args)]
struct GenerateKeyArgs {
    /// Directory that will receive private.pem and jwks.json.
    #[arg(long, default_value = DEFAULT_OUTPUT_DIR)]
    output_dir: PathBuf,

    /// Key id stored in the token header and public JWK.
    #[arg(long, default_value = DEFAULT_KEY_ID)]
    kid: String,

    /// RSA modulus size in bits.
    #[arg(long, default_value_t = DEFAULT_RSA_BITS)]
    bits: usize,
}

#[derive(Debug)]
struct GeneratedKeyFiles {
    private_key_path: PathBuf,
    jwks_path: PathBuf,
}

pub fn run(args: InternalContextArgs) -> io::Result<()> {
    match args.command {
        InternalContextCommand::GenerateKey(args) => {
            let generated = generate_key_files(&args.output_dir, &args.kid, args.bits)?;
            println!("Generated internal-context signing material:");
            println!("  private key: {}", generated.private_key_path.display());
            println!("  public JWKS: {}", generated.jwks_path.display());
            println!();
            println!("Configure Stargate with:");
            println!("INTERNAL_CONTEXT_ALGORITHM=RS256");
            println!("INTERNAL_CONTEXT_KID={}", args.kid);
            println!(
                "INTERNAL_CONTEXT_PRIVATE_KEY_PATH={}",
                generated.private_key_path.display()
            );
            println!(
                "INTERNAL_CONTEXT_JWKS_PATH={}",
                generated.jwks_path.display()
            );
            Ok(())
        }
    }
}

fn generate_key_files(
    output_dir: &Path,
    key_id: &str,
    bits: usize,
) -> io::Result<GeneratedKeyFiles> {
    validate_generation_input(key_id, bits)?;

    let private_key_path = output_dir.join("private.pem");
    let jwks_path = output_dir.join("jwks.json");
    reject_existing_output(&private_key_path)?;
    reject_existing_output(&jwks_path)?;

    fs::create_dir_all(output_dir)?;
    set_directory_permissions(output_dir)?;

    let (private_pem, _) =
        ::jwt::generate_rsa_keys(bits).map_err(|error| io::Error::other(error.to_string()))?;
    let private_key = RsaPrivateKey::from_pkcs8_pem(&private_pem)
        .map_err(|error| io::Error::other(format!("generated private key is invalid: {error}")))?;
    let public_jwk = serde_json::json!({
        "kty": "RSA",
        "use": "sig",
        "key_ops": ["verify"],
        "alg": "RS256",
        "kid": key_id,
        "n": URL_SAFE_NO_PAD.encode(private_key.n().to_bytes_be()),
        "e": URL_SAFE_NO_PAD.encode(private_key.e().to_bytes_be()),
    });
    let public_jwk_bytes =
        serde_json::to_vec(&public_jwk).map_err(|error| io::Error::other(error.to_string()))?;
    ctx::VerificationKey::from_jwk_json(&public_jwk_bytes)
        .map_err(|error| io::Error::other(format!("generated public JWK is invalid: {error}")))?;

    let mut jwks_bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "keys": [public_jwk]
    }))
    .map_err(|error| io::Error::other(error.to_string()))?;
    jwks_bytes.push(b'\n');

    write_new_pair(
        &private_key_path,
        private_pem.as_bytes(),
        &jwks_path,
        &jwks_bytes,
    )?;

    Ok(GeneratedKeyFiles {
        private_key_path,
        jwks_path,
    })
}

fn validate_generation_input(key_id: &str, bits: usize) -> io::Result<()> {
    if key_id.trim().is_empty()
        || key_id.len() > ctx::MAX_KEY_ID_BYTES
        || !key_id.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "--kid must be nonblank printable ASCII and at most {} bytes",
                ctx::MAX_KEY_ID_BYTES
            ),
        ));
    }
    if !(MIN_RSA_BITS..=MAX_RSA_BITS).contains(&bits) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("--bits must be between {MIN_RSA_BITS} and {MAX_RSA_BITS}"),
        ));
    }
    Ok(())
}

fn reject_existing_output(path: &Path) -> io::Result<()> {
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("refusing to overwrite '{}'", path.display()),
        ));
    }
    Ok(())
}

fn write_new_pair(
    private_key_path: &Path,
    private_key: &[u8],
    jwks_path: &Path,
    jwks: &[u8],
) -> io::Result<()> {
    let mut private_file = open_new(private_key_path, 0o600)?;
    let mut jwks_file = match open_new(jwks_path, 0o644) {
        Ok(file) => file,
        Err(error) => {
            drop(private_file);
            let _ = fs::remove_file(private_key_path);
            return Err(error);
        }
    };

    let result = (|| {
        private_file.write_all(private_key)?;
        private_file.sync_all()?;
        jwks_file.write_all(jwks)?;
        jwks_file.sync_all()
    })();
    drop(private_file);
    drop(jwks_file);

    if result.is_err() {
        let _ = fs::remove_file(private_key_path);
        let _ = fs::remove_file(jwks_path);
    }
    result
}

fn open_new(path: &Path, mode: u32) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    set_creation_mode(&mut options, mode);
    options.open(path)
}

#[cfg(unix)]
fn set_creation_mode(options: &mut OpenOptions, mode: u32) {
    use std::os::unix::fs::OpenOptionsExt as _;
    options.mode(mode);
}

#[cfg(not(unix))]
fn set_creation_mode(_options: &mut OpenOptions, _mode: u32) {}

#[cfg(unix)]
fn set_directory_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_directory_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::BigUint;

    fn temporary_output_dir(test_name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("stargate-{test_name}-{}", ulid::Ulid::generate()))
    }

    #[test]
    fn generated_files_are_matching_and_verifier_compatible() {
        let output_dir = temporary_output_dir("internal-context-keygen");
        let generated =
            generate_key_files(&output_dir, "stargate-internal-test", DEFAULT_RSA_BITS).unwrap();

        let private_pem = fs::read_to_string(&generated.private_key_path).unwrap();
        let private_key = RsaPrivateKey::from_pkcs8_pem(&private_pem).unwrap();
        let jwks: serde_json::Value =
            serde_json::from_slice(&fs::read(&generated.jwks_path).unwrap()).unwrap();
        let jwk = &jwks["keys"][0];
        let jwk_bytes = serde_json::to_vec(jwk).unwrap();
        ctx::VerificationKey::from_jwk_json(&jwk_bytes).unwrap();
        let decode_part = |name: &str| {
            BigUint::from_bytes_be(&URL_SAFE_NO_PAD.decode(jwk[name].as_str().unwrap()).unwrap())
        };

        assert_eq!(jwk["kid"], "stargate-internal-test");
        assert_eq!(decode_part("n"), *private_key.n());
        assert_eq!(decode_part("e"), *private_key.e());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(&generated.private_key_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(&output_dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(&generated.jwks_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o644
            );
        }

        fs::remove_dir_all(output_dir).unwrap();
    }

    #[test]
    fn generation_refuses_invalid_inputs_and_existing_files() {
        let output_dir = temporary_output_dir("internal-context-keygen-errors");
        fs::create_dir_all(&output_dir).unwrap();
        fs::write(output_dir.join("private.pem"), "keep-me").unwrap();

        let error = generate_key_files(&output_dir, "stargate-internal-test", DEFAULT_RSA_BITS)
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(output_dir.join("private.pem")).unwrap(),
            "keep-me"
        );
        assert_eq!(
            validate_generation_input("invalid key id", DEFAULT_RSA_BITS)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            validate_generation_input("valid-key", MIN_RSA_BITS - 1)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );

        fs::remove_dir_all(output_dir).unwrap();
    }
}
