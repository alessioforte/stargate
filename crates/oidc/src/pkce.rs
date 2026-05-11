use crate::OAuthError;
use crate::codes::AuthorizationCodeRecord;
use crate::scopes::PKCE_METHOD_S256;
use base64::Engine;
use sha2::{Digest, Sha256};

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

pub fn verify(code: &AuthorizationCodeRecord, verifier: &str) -> Result<(), OAuthError> {
    if code.code_challenge_method != PKCE_METHOD_S256 {
        return Err(OAuthError::invalid_grant("unsupported code challenge"));
    }

    if !valid_code_verifier(verifier) {
        return Err(OAuthError::invalid_grant("invalid code_verifier"));
    }

    if s256_challenge(verifier) != code.code_challenge {
        return Err(OAuthError::invalid_grant("invalid code_verifier"));
    }

    Ok(())
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
    fn verifier_rejects_invalid_values() {
        assert!(!valid_code_verifier("short"));
        assert!(!valid_code_verifier(
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk!"
        ));
    }
}
