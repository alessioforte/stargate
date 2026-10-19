//! File layout of the JWT signing keys (asymmetric algorithms).
//!
//! ```text
//! .stargate/jwks/
//! ├── keys/<kid>/private.pem   signing key (required for the active key)
//! ├── keys/<kid>/public.pem    verification key (required)
//! ├── keys/<kid>/alg           optional, e.g. "ES256"; inferred otherwise
//! └── private.pem, public.pem  legacy single-key layout (es256/, es384/ for EC)
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
//!
//! Installations created before the keyring keep their legacy files as the
//! active key until `stargate jwt generate-key` moves them into
//! `keys/<current kid>/`.

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

/// The loaded keyring: the active signing key and verification-only keys.
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

/// Where the pre-keyring layout keeps the pair for `algorithm`.
fn legacy_pair(root: &Path, algorithm: Algorithm) -> (PathBuf, PathBuf) {
    let dir = match algorithm {
        Algorithm::ES256 => root.join("es256"),
        Algorithm::ES384 => root.join("es384"),
        _ => root.to_path_buf(),
    };
    (dir.join(PRIVATE_KEY), dir.join(PUBLIC_KEY))
}

fn both_exist((private, public): &(PathBuf, PathBuf)) -> bool {
    private.is_file() && public.is_file()
}

/// Algorithm of keys that sign OIDC ID tokens (`JWT_ID_TOKEN_KID`):
/// OpenID Connect requires providers to support RS256.
pub(crate) const ID_TOKEN_ALGORITHM: Algorithm = Algorithm::RS256;

/// Load the keyring under `root` for an asymmetric active key.
///
/// `configured` is `JWT_ALGORITHM` when set: it must match the active key's
/// algorithm, and picks the algorithm of a generated active key (default
/// RS256). `id_token_key_id` (`JWT_ID_TOKEN_KID`) names a key loaded with its
/// private key so it can sign ID tokens.
///
/// Missing keys are generated only on a fresh keyring (no legacy files and
/// no key directories besides the two named ones). Otherwise a missing key is
/// an error: silently creating it would sign with a key nobody has seen.
pub(crate) fn load(
    root: &Path,
    active_key_id: &str,
    id_token_key_id: Option<&str>,
    configured: Option<Algorithm>,
) -> Result<LoadedKeyring, String> {
    jwt::validate_key_id(active_key_id).map_err(|error| format!("JWT_KID: {error}"))?;
    let default_algorithm = configured.unwrap_or(Algorithm::RS256);

    let active_dir = key_dir(root, active_key_id);
    let legacy = legacy_pair(root, default_algorithm);
    let fresh = is_fresh(root, &[Some(active_key_id), id_token_key_id])?;
    let active = if active_dir.is_dir() {
        if both_exist(&legacy) {
            tracing::warn!(
                path = %legacy.0.display(),
                "legacy JWT key files are ignored because keys/{active_key_id} exists"
            );
        }
        load_dir(&active_dir, active_key_id, true, configured)?
    } else if both_exist(&legacy) {
        load_pair(active_key_id, default_algorithm, Some(&legacy.0), &legacy.1)?
    } else if fresh {
        // First start: create the active key in the keyring layout.
        generate(root, active_key_id, default_algorithm).map_err(|error| error.to_string())?;
        load_dir(&active_dir, active_key_id, true, configured)?
    } else {
        return Err(missing_key(
            "JWT_KID",
            active_key_id,
            default_algorithm,
            root,
        ));
    };

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
        if Some(key_id) != skip && !key_dir(root, key_id).is_dir() {
            if !fresh {
                return Err(missing_key(
                    "JWT_ID_TOKEN_KID",
                    key_id,
                    ID_TOKEN_ALGORITHM,
                    root,
                ));
            }
            generate(root, key_id, ID_TOKEN_ALGORITHM).map_err(|error| error.to_string())?;
        }
    }

    let mut keys = Vec::new();
    for (key_id, path) in key_dirs(root)? {
        if Some(key_id.as_str()) == skip {
            continue;
        }
        let signs_id_tokens = Some(key_id.as_str()) == id_token_key_id;
        keys.push(load_dir(&path, &key_id, signs_id_tokens, None)?);
    }
    Ok(keys)
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

/// No legacy key files and no key directories other than `named` ones.
fn is_fresh(root: &Path, named: &[Option<&str>]) -> Result<bool, String> {
    let legacy = [Algorithm::RS256, Algorithm::ES256, Algorithm::ES384]
        .into_iter()
        .any(|algorithm| both_exist(&legacy_pair(root, algorithm)));
    if legacy {
        return Ok(false);
    }
    Ok(key_dirs(root)?
        .iter()
        .all(|(key_id, _)| named.contains(&Some(key_id.as_str()))))
}

fn missing_key(variable: &str, key_id: &str, algorithm: Algorithm, root: &Path) -> String {
    let id_token_flag = if variable == "JWT_ID_TOKEN_KID" {
        " --id-token"
    } else {
        ""
    };
    format!(
        "{variable} names '{key_id}', which is not in {}; create it first with \
         `stargate jwt generate-key --kid {key_id}{id_token_flag} --algorithm {algorithm:?}` \
         (a legacy ES key also needs JWT_ALGORITHM set to its algorithm)",
        root.join(KEYS_DIR).display()
    )
}

/// Load `keys/<kid>/`. Only signing keys (`signs`: the active key and the ID
/// token key) read their private key; other keys only verify, so their
/// private keys may be deleted.
fn load_dir(
    dir: &Path,
    key_id: &str,
    signs: bool,
    configured: Option<Algorithm>,
) -> Result<KeyMaterial, String> {
    jwt::validate_key_id(key_id).map_err(|error| format!("{}: {error}", dir.display()))?;
    let public = dir.join(PUBLIC_KEY);
    let public_pem = fs::read(&public).map_err(|error| format!("{}: {error}", public.display()))?;
    let inferred = jwt::infer_algorithm(&public_pem)
        .map_err(|error| format!("{}: {error}", public.display()))?;
    let algorithm = match read_algorithm(dir)? {
        Some(algorithm) => algorithm,
        // Without `alg`, an RSA key uses the configured RSA variant.
        None => configured
            .filter(|configured| same_family(*configured, inferred))
            .unwrap_or(inferred),
    };
    let private = dir.join(PRIVATE_KEY);
    load_pair(
        key_id,
        algorithm,
        signs.then_some(private.as_path()),
        &public,
    )
}

fn same_family(left: Algorithm, right: Algorithm) -> bool {
    let rsa = |algorithm| {
        matches!(
            algorithm,
            Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512
        )
    };
    left == right || (rsa(left) && rsa(right))
}

fn load_pair(
    key_id: &str,
    algorithm: Algorithm,
    private: Option<&Path>,
    public: &Path,
) -> Result<KeyMaterial, String> {
    let public_pem = fs::read(public).map_err(|error| format!("{}: {error}", public.display()))?;
    let private_pem = private
        .map(|path| fs::read(path).map_err(|error| format!("{}: {error}", path.display())))
        .transpose()?;
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

/// Move the legacy pair for `algorithm` into `keys/<kid>/` so it stays
/// published and accepted once another key is active. Returns the new
/// directory, or `None` when there is nothing to move.
pub(crate) fn adopt_legacy(
    root: &Path,
    key_id: &str,
    algorithm: Algorithm,
) -> io::Result<Option<PathBuf>> {
    let (private, public) = legacy_pair(root, algorithm);
    let dir = key_dir(root, key_id);
    if !private.is_file() || !public.is_file() || dir.exists() {
        return Ok(None);
    }
    jwt::validate_key_id(key_id)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    // Fail before moving anything if the legacy pair is unusable.
    load_pair(key_id, algorithm, Some(&private), &public)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    create_key_dir(&dir)?;
    fs::rename(&private, dir.join(PRIVATE_KEY))?;
    fs::rename(&public, dir.join(PUBLIC_KEY))?;
    write_new(
        &dir.join(ALGORITHM_FILE),
        format!("{algorithm:?}\n").as_bytes(),
        0o644,
    )?;
    Ok(Some(dir))
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

    fn write_legacy(root: &Path) {
        let (private, public) = jwt::generate_rsa_keys(2048).unwrap();
        fs::create_dir_all(root).unwrap();
        fs::write(root.join(PRIVATE_KEY), private).unwrap();
        fs::write(root.join(PUBLIC_KEY), public).unwrap();
    }

    #[test]
    fn first_start_generates_the_active_key_in_the_keyring() {
        let root = temp_root("jwt-keyring-first");
        let keyring = load(&root, "k1", None, None).unwrap();

        assert_eq!(keyring.active.key_id(), Some("k1"));
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
    fn legacy_layout_stays_active_until_adopted() {
        let root = temp_root("jwt-keyring-legacy");
        write_legacy(&root);

        let keyring = load(&root, "stargate-current", None, None).unwrap();
        assert_eq!(keyring.active.key_id(), Some("stargate-current"));
        assert!(!root.join(KEYS_DIR).exists(), "no files created");

        let adopted = adopt_legacy(&root, "stargate-current", Algorithm::RS256).unwrap();
        assert_eq!(adopted, Some(root.join("keys/stargate-current")));
        assert!(!root.join(PRIVATE_KEY).exists());
        assert!(
            adopt_legacy(&root, "stargate-current", Algorithm::RS256)
                .unwrap()
                .is_none()
        );
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
        assert_eq!(prepublished.active.key_id(), Some("k1"));
        assert_eq!(prepublished.additional.len(), 1);
        assert_eq!(prepublished.additional[0].key_id(), Some("k2"));
        assert!(!prepublished.additional[0].can_sign());

        let rotated = load(&root, "k2", None, None).unwrap();
        assert_eq!(rotated.active.algorithm(), Algorithm::ES256);
        assert_eq!(rotated.additional[0].key_id(), Some("k1"));

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
        assert_eq!(id_key.key_id(), Some("rsa-1"));
        assert_eq!(id_key.algorithm(), ID_TOKEN_ALGORITHM);
        assert!(id_key.can_sign(), "ID token key loads its private key");

        // Other verification-only keys still skip their private key.
        generate(&root, "next", Algorithm::ES256).unwrap();
        let keyring = load(&root, "ec-1", Some("rsa-1"), Some(Algorithm::ES256)).unwrap();
        let next = keyring
            .additional
            .iter()
            .find(|key| key.key_id() == Some("next"))
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
    fn legacy_files_of_another_algorithm_are_not_replaced() {
        let root = temp_root("jwt-keyring-legacy-ec");
        let (private, public) = jwt::generate_p256_keys().unwrap();
        fs::create_dir_all(root.join("es256")).unwrap();
        fs::write(root.join("es256").join(PRIVATE_KEY), private).unwrap();
        fs::write(root.join("es256").join(PUBLIC_KEY), public).unwrap();

        // JWT_ALGORITHM unset (RS256) does not see the ES256 legacy pair.
        assert!(load(&root, "stargate-current", None, None).is_err());
        assert!(!root.join(KEYS_DIR).exists(), "no RSA key generated");
        assert!(load(&root, "stargate-current", None, Some(Algorithm::ES256)).is_ok());
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
