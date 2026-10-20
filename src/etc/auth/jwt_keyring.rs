//! File layout of the JWT signing keys (asymmetric algorithms).
//!
//! ```text
//! .stargate/jwks/keys/<kid>/private.pem   signing key (needed by signing keys only)
//! .stargate/jwks/keys/<kid>/public.pem    verification key
//! .stargate/jwks/keys/<kid>/alg           e.g. "ES256"; inferred from the key if absent
//! ```
//!
//! `JWT_KID` names the active key. Every other directory under `keys/` is a
//! verification-only key: published in the JWKS and accepted for tokens that
//! carry its `kid`. That covers both the next key (published before it is
//! activated) and retired keys (kept until their tokens expire).
//!
//! `JWT_ID_TOKEN_KID` optionally names an RS256 key that signs OIDC ID tokens
//! while the active key (e.g. ES256, or an HMAC secret) signs everything else.
//! OpenID Connect requires RS256 ID tokens to be supported.

use jwt::{Algorithm, KeyMaterial};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub(crate) const JWKS_DIR: &str = ".stargate/jwks";
const KEYS_DIR: &str = "keys";
const PRIVATE_KEY: &str = "private.pem";
const PUBLIC_KEY: &str = "public.pem";
const ALGORITHM_FILE: &str = "alg";
const RSA_KEY_BITS: usize = 2048;

/// Algorithm of keys that sign OIDC ID tokens (`JWT_ID_TOKEN_KID`):
/// OpenID Connect requires providers to support RS256.
pub(crate) const ID_TOKEN_ALGORITHM: Algorithm = Algorithm::RS256;

/// The loaded keyring: the active signing key and the other keys.
pub(crate) struct LoadedKeyring {
    pub(crate) active: KeyMaterial,
    pub(crate) additional: Vec<KeyMaterial>,
}

pub(crate) fn is_asymmetric(algorithm: Algorithm) -> bool {
    matches!(
        algorithm,
        Algorithm::RS256
            | Algorithm::RS384
            | Algorithm::RS512
            | Algorithm::ES256
            | Algorithm::ES384
    )
}

fn key_dir(root: &Path, key_id: &str) -> PathBuf {
    root.join(KEYS_DIR).join(key_id)
}

/// Load the keyring under `root` for an asymmetric active key.
///
/// `configured` is `JWT_ALGORITHM` when set: it must match the active key's
/// algorithm, and picks the algorithm of a generated active key (default
/// RS256). `id_token_key_id` (`JWT_ID_TOKEN_KID`) names a key loaded with its
/// private key so it can sign ID tokens.
///
/// Missing keys are generated only on a fresh keyring (no key directories
/// besides the two named ones). Otherwise a missing key is an error: silently
/// creating it would sign with a key nobody has seen.
pub(crate) fn load(
    root: &Path,
    active_key_id: &str,
    id_token_key_id: Option<&str>,
    configured: Option<Algorithm>,
) -> Result<LoadedKeyring, String> {
    jwt::validate_key_id(active_key_id).map_err(|error| format!("JWT_KID: {error}"))?;
    let fresh = is_fresh(root, &[Some(active_key_id), id_token_key_id])?;
    let algorithm = configured.unwrap_or(Algorithm::RS256);
    ensure_key("JWT_KID", root, active_key_id, algorithm, fresh)?;

    let active = load_dir(&key_dir(root, active_key_id), active_key_id, true)?;
    if let Some(configured) = configured
        && configured != active.algorithm()
    {
        return Err(format!(
            "JWT_ALGORITHM is {configured:?} but the active key '{active_key_id}' is {:?}",
            active.algorithm()
        ));
    }

    let additional = load_others(root, Some(active_key_id), id_token_key_id, fresh)?;
    Ok(LoadedKeyring { active, additional })
}

/// Keys for an HMAC configuration that signs ID tokens with
/// `id_token_key_id`: that key (with its private key) plus the other keyring
/// keys, verification-only, for rotation overlap.
pub(crate) fn load_for_id_tokens(
    root: &Path,
    id_token_key_id: &str,
) -> Result<Vec<KeyMaterial>, String> {
    let fresh = is_fresh(root, &[Some(id_token_key_id)])?;
    load_others(root, None, Some(id_token_key_id), fresh)
}

/// Every key directory except `skip`. The ID token key is loaded with its
/// private key, and generated (RS256) on a fresh keyring.
fn load_others(
    root: &Path,
    skip: Option<&str>,
    id_token_key_id: Option<&str>,
    fresh: bool,
) -> Result<Vec<KeyMaterial>, String> {
    if let Some(key_id) = id_token_key_id {
        jwt::validate_key_id(key_id).map_err(|error| format!("JWT_ID_TOKEN_KID: {error}"))?;
        ensure_key("JWT_ID_TOKEN_KID", root, key_id, ID_TOKEN_ALGORITHM, fresh)?;
    }

    let mut keys = Vec::new();
    for (key_id, path) in key_dirs(root)? {
        if Some(key_id.as_str()) == skip {
            continue;
        }
        let signs_id_tokens = Some(key_id.as_str()) == id_token_key_id;
        keys.push(load_dir(&path, &key_id, signs_id_tokens)?);
    }
    Ok(keys)
}

/// Generate the key named by `variable` when it is missing from a fresh
/// keyring; a missing key on a keyring in use is an error.
fn ensure_key(
    variable: &str,
    root: &Path,
    key_id: &str,
    algorithm: Algorithm,
    fresh: bool,
) -> Result<(), String> {
    if key_dir(root, key_id).is_dir() {
        return Ok(());
    }
    if !fresh {
        let id_token_flag = if variable == "JWT_ID_TOKEN_KID" {
            " --id-token"
        } else {
            ""
        };
        return Err(format!(
            "{variable} names '{key_id}', which is not in {}; create it first with \
             `stargate jwt generate-key --kid {key_id}{id_token_flag} --algorithm {algorithm:?}`",
            root.join(KEYS_DIR).display()
        ));
    }
    generate(root, key_id, algorithm)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// `(kid, path)` of every directory under `keys/`, sorted by kid.
fn key_dirs(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let keys_dir = root.join(KEYS_DIR);
    if !keys_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries = fs::read_dir(&keys_dir)
        .map_err(|error| format!("{}: {error}", keys_dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("{}: {error}", keys_dir.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut dirs = Vec::new();
    for entry in entries {
        if !entry.path().is_dir() {
            continue;
        }
        let Some(key_id) = entry.file_name().to_str().map(str::to_string) else {
            return Err(format!(
                "{}: key directory names must be UTF-8",
                keys_dir.display()
            ));
        };
        dirs.push((key_id, entry.path()));
    }
    Ok(dirs)
}

/// No key directories other than the `named` ones.
fn is_fresh(root: &Path, named: &[Option<&str>]) -> Result<bool, String> {
    Ok(key_dirs(root)?
        .iter()
        .all(|(key_id, _)| named.contains(&Some(key_id.as_str()))))
}

/// Load `keys/<kid>/`. Only signing keys (`signs`: the active key and the ID
/// token key) read their private key; other keys only verify, so their
/// private keys may be deleted.
fn load_dir(dir: &Path, key_id: &str, signs: bool) -> Result<KeyMaterial, String> {
    jwt::validate_key_id(key_id).map_err(|error| format!("{}: {error}", dir.display()))?;
    let read = |path: &Path| fs::read(path).map_err(|error| format!("{}: {error}", path.display()));
    let public = dir.join(PUBLIC_KEY);
    let public_pem = read(&public)?;
    let algorithm = match read_algorithm(dir)? {
        Some(algorithm) => algorithm,
        None => jwt::infer_algorithm(&public_pem)
            .map_err(|error| format!("{}: {error}", public.display()))?,
    };
    let private_pem = signs.then(|| read(&dir.join(PRIVATE_KEY))).transpose()?;
    KeyMaterial::from_pem(key_id, algorithm, private_pem.as_deref(), &public_pem)
        .map_err(|error| error.to_string())
}

fn read_algorithm(dir: &Path) -> Result<Option<Algorithm>, String> {
    let path = dir.join(ALGORITHM_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let algorithm = Algorithm::from_str(text.trim())
        .ok()
        .filter(|algorithm| is_asymmetric(*algorithm))
        .ok_or_else(|| {
            format!(
                "{}: '{}' is not RS256, RS384, RS512, ES256, or ES384",
                path.display(),
                text.trim()
            )
        })?;
    Ok(Some(algorithm))
}

/// Create `keys/<kid>/` with a new key pair. Refuses to overwrite.
pub(crate) fn generate(root: &Path, key_id: &str, algorithm: Algorithm) -> io::Result<PathBuf> {
    jwt::validate_key_id(key_id)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let (private_pem, public_pem) = match algorithm {
        Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
            jwt::generate_rsa_keys(RSA_KEY_BITS)
        }
        Algorithm::ES256 => jwt::generate_p256_keys(),
        Algorithm::ES384 => jwt::generate_p384_keys(),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{algorithm:?} keys cannot be generated; use RS256, RS384, RS512, ES256, or ES384"),
            ));
        }
    }
    .map_err(|error| io::Error::other(error.to_string()))?;

    let dir = key_dir(root, key_id);
    create_key_dir(&dir)?;
    write_new(&dir.join(PRIVATE_KEY), private_pem.as_bytes(), 0o600)?;
    write_new(&dir.join(PUBLIC_KEY), public_pem.as_bytes(), 0o644)?;
    write_new(
        &dir.join(ALGORITHM_FILE),
        format!("{algorithm:?}\n").as_bytes(),
        0o644,
    )?;
    Ok(dir)
}

fn create_key_dir(dir: &Path) -> io::Result<()> {
    if dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("refusing to overwrite '{}'", dir.display()),
        ));
    }
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn write_new(path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file: File = options.open(path)?;
    file.write_all(contents)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("stargate-{name}-{}", ulid::Ulid::generate()))
    }

    #[test]
    fn first_start_generates_the_active_key_in_the_keyring() {
        let root = temp_root("jwt-keyring-first");
        let keyring = load(&root, "k1", None, None).unwrap();

        assert_eq!(keyring.active.key_id(), "k1");
        assert_eq!(keyring.active.algorithm(), Algorithm::RS256);
        assert!(keyring.additional.is_empty());
        assert!(root.join("keys/k1/private.pem").is_file());
        assert_eq!(
            fs::read_to_string(root.join("keys/k1/alg")).unwrap(),
            "RS256\n"
        );

        // The same key is loaded again on the next start.
        let again = load(&root, "k1", None, None).unwrap();
        assert!(again.active.can_sign());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rotation_publishes_next_key_then_keeps_the_retired_one() {
        let root = temp_root("jwt-keyring-rotation");
        load(&root, "k1", None, None).unwrap();
        generate(&root, "k2", Algorithm::ES256).unwrap();
        assert!(
            generate(&root, "k2", Algorithm::ES256).is_err(),
            "no overwrite"
        );

        let prepublished = load(&root, "k1", None, None).unwrap();
        assert_eq!(prepublished.active.key_id(), "k1");
        assert_eq!(prepublished.additional.len(), 1);
        assert_eq!(prepublished.additional[0].key_id(), "k2");
        assert!(!prepublished.additional[0].can_sign());

        let rotated = load(&root, "k2", None, None).unwrap();
        assert_eq!(rotated.active.algorithm(), Algorithm::ES256);
        assert_eq!(rotated.additional[0].key_id(), "k1");

        // A retired key may drop its private key and still verify.
        fs::remove_file(root.join("keys/k1/private.pem")).unwrap();
        assert_eq!(load(&root, "k2", None, None).unwrap().additional.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn configured_algorithm_must_match_the_active_key() {
        let root = temp_root("jwt-keyring-algorithm");
        generate(&root, "k1", Algorithm::ES256).unwrap();

        assert!(load(&root, "k1", None, Some(Algorithm::RS256)).is_err());
        assert!(load(&root, "k1", None, Some(Algorithm::ES256)).is_ok());
        assert!(load(&root, "../k1", None, None).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fresh_keyring_generates_the_active_and_id_token_keys() {
        let root = temp_root("jwt-keyring-id-fresh");
        let keyring = load(&root, "ec-1", Some("rsa-1"), Some(Algorithm::ES256)).unwrap();

        assert_eq!(keyring.active.algorithm(), Algorithm::ES256);
        assert_eq!(keyring.additional.len(), 1);
        let id_key = &keyring.additional[0];
        assert_eq!(id_key.key_id(), "rsa-1");
        assert_eq!(id_key.algorithm(), ID_TOKEN_ALGORITHM);
        assert!(id_key.can_sign(), "ID token key loads its private key");

        // Other verification-only keys still skip their private key.
        generate(&root, "next", Algorithm::ES256).unwrap();
        let keyring = load(&root, "ec-1", Some("rsa-1"), Some(Algorithm::ES256)).unwrap();
        let next = keyring
            .additional
            .iter()
            .find(|key| key.key_id() == "next")
            .unwrap();
        assert!(!next.can_sign());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_keys_are_errors_once_the_keyring_is_in_use() {
        let root = temp_root("jwt-keyring-missing");
        load(&root, "k1", None, None).unwrap();

        // A typo in JWT_KID must not mint a key nobody has published.
        let error = load(&root, "k2", None, None).err().unwrap();
        assert!(
            error.contains("stargate jwt generate-key --kid k2"),
            "{error}"
        );
        assert!(!root.join("keys/k2").exists());

        // Enabling an ID token key on a keyring with only the active key
        // creates it; once other keys exist, a missing one is an error.
        load(&root, "k1", Some("rsa-1"), None).unwrap();
        assert!(root.join("keys/rsa-1/private.pem").is_file());
        let error = load(&root, "k1", Some("rsa-2"), None).err().unwrap();
        assert!(error.contains("--id-token"), "{error}");

        // The active key may also sign ID tokens.
        assert!(load(&root, "k1", Some("k1"), None).is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hmac_configurations_load_the_id_token_key() {
        let root = temp_root("jwt-keyring-hmac");
        let keys = load_for_id_tokens(&root, "rsa-1").unwrap();

        assert_eq!(keys.len(), 1);
        assert!(keys[0].can_sign());
        assert_eq!(keys[0].algorithm(), Algorithm::RS256);
        fs::remove_dir_all(root).unwrap();
    }
}
