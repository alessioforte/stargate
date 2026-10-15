use crate::OAuthError;
use crate::codes::AuthorizationCodeRecord;
use crate::scopes::PKCE_METHOD_S256;
use base64::Engine;
use sha2::{Digest, Sha256};

/// Whether an authorization request from this client must carry a PKCE
/// challenge. Public clients always need one; confidential clients default to
/// requiring it (OAuth 2.1) but can opt out with `attrs.require_pkce = false`
/// (or `requirePkce`), since OpenID Connect Core does not mandate PKCE.
pub fn client_requires_pkce(public_client: bool, attrs: &serde_json::Value) -> bool {
    if public_client {
        return true;
    }
    ["require_pkce", "requirePkce"]
        .into_iter()
        .find_map(|key| attrs.get(key).and_then(serde_json::Value::as_bool))
        .unwrap_or(true)
}

pub fn valid_code_verifier(verifier: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && verifier.bytes().all(|byte| {
            matches!(
                byte,
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~'
            )
        })
}

pub fn s256_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&digest[..])
}

/// Check the token request's `code_verifier` against the code's challenge.
/// A code issued without a challenge must not be redeemed with a verifier,
/// which blocks PKCE downgrade attempts.
pub fn verify(code: &AuthorizationCodeRecord, verifier: Option<&str>) -> Result<(), OAuthError> {
    if code.code_challenge.is_empty() {
        return match verifier {
            None => Ok(()),
            Some(_) => Err(OAuthError::invalid_grant(
                "code_verifier sent for a code issued without code_challenge",
            )),
        };
    }

    let Some(verifier) = verifier else {
        return Err(OAuthError::invalid_request("code_verifier is required"));
    };

    if code.code_challenge_method != PKCE_METHOD_S256 {
        return Err(OAuthError::invalid_grant("unsupported code challenge"));
    }

    if !valid_code_verifier(verifier) {
        return Err(OAuthError::invalid_grant("invalid code_verifier"));
    }

    if !constant_time_eq(
        s256_challenge(verifier).as_bytes(),
        code.code_challenge.as_bytes(),
    ) {
        return Err(OAuthError::invalid_grant("invalid code_verifier"));
    }

    Ok(())
}

/// Compare two byte slices without early-exit on the first differing byte.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s256_challenge_matches_rfc_example() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

        assert_eq!(
            s256_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert!(valid_code_verifier(verifier));
    }

    #[test]
    fn confidential_clients_require_pkce_unless_opted_out() {
        assert!(client_requires_pkce(
            true,
            &serde_json::json!({ "require_pkce": false })
        ));
        assert!(client_requires_pkce(false, &serde_json::json!({})));
        assert!(!client_requires_pkce(
            false,
            &serde_json::json!({ "require_pkce": false })
        ));
        assert!(!client_requires_pkce(
            false,
            &serde_json::json!({ "requirePkce": false })
        ));
    }

    #[test]
    fn verify_rejects_verifier_for_code_without_challenge() {
        let mut code = crate::codes::tests_support::record();
        code.code_challenge = String::new();
        code.code_challenge_method = String::new();

        assert!(verify(&code, None).is_ok());
        assert!(verify(&code, Some("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")).is_err());
    }

    #[test]
    fn verify_requires_verifier_for_code_with_challenge() {
        let code = crate::codes::tests_support::record();

        assert!(verify(&code, None).is_err());
        assert!(verify(&code, Some("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")).is_ok());
    }

    #[test]
    fn verifier_rejects_invalid_values() {
        assert!(!valid_code_verifier("short"));
        assert!(!valid_code_verifier(
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk!"
        ));
    }
}
