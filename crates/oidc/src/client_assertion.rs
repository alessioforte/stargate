//! `private_key_jwt` client authentication (OIDC Core 9, RFC 7523 §3): the
//! client signs a short-lived JWT with its own key and sends it as
//! `client_assertion`.

use serde::Deserialize;

/// The only `client_assertion_type` (RFC 7523 §2.2).
pub const CLIENT_ASSERTION_TYPE_JWT_BEARER: &str =
    "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";

/// Longest accepted assertion lifetime; replay markers last as long.
pub const MAX_ASSERTION_LIFETIME_SECS: i64 = 3600;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize)]
pub struct ClientAssertionClaims {
    iss: String,
    sub: String,
    aud: Audience,
    exp: i64,
    jti: Option<String>,
}

/// What replay protection needs from a verified assertion.
#[derive(Debug, PartialEq, Eq)]
pub struct AcceptedAssertion {
    pub jti: String,
    pub exp: i64,
}

/// The client an assertion claims to come from (`sub`, else `iss`), read
/// without verifying it, to find the keys that verify it.
pub fn claimed_client_id(assertion: &str) -> Option<String> {
    let claims = crate::request_object::unverified_claims(assertion)?;
    claims
        .get("sub")
        .or_else(|| claims.get("iss"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

/// Check the claims of a signature-verified assertion: issued by and about
/// `client_id`, for one of `audiences` (the issuer or an endpoint URL), with
/// a `jti`, and not valid for longer than [`MAX_ASSERTION_LIFETIME_SECS`].
pub fn accept(
    claims: ClientAssertionClaims,
    client_id: &str,
    audiences: &[String],
    now: i64,
) -> Result<AcceptedAssertion, &'static str> {
    if claims.iss != client_id || claims.sub != client_id {
        return Err("client_assertion iss and sub must be the client_id");
    }
    let audience_ok = match &claims.aud {
        Audience::One(value) => audiences.contains(value),
        Audience::Many(values) => values.iter().any(|value| audiences.contains(value)),
    };
    if !audience_ok {
        return Err("client_assertion aud must be the issuer or the endpoint URL");
    }
    if claims.exp - now > MAX_ASSERTION_LIFETIME_SECS {
        return Err("client_assertion lives too long (more than one hour)");
    }
    let jti = claims
        .jti
        .filter(|jti| !jti.trim().is_empty())
        .ok_or("client_assertion jti is required")?;
    Ok(AcceptedAssertion {
        jti,
        exp: claims.exp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    fn claims(value: serde_json::Value) -> ClientAssertionClaims {
        serde_json::from_value(value).unwrap()
    }

    fn audiences() -> Vec<String> {
        vec![
            "https://id.example.com".to_string(),
            "https://id.example.com/oauth/token".to_string(),
        ]
    }

    #[test]
    fn accepts_a_well_formed_assertion() {
        let accepted = accept(
            claims(serde_json::json!({
                "iss": "c1", "sub": "c1", "aud": ["https://id.example.com/oauth/token", "x"],
                "exp": 160, "jti": "j1"
            })),
            "c1",
            &audiences(),
            100,
        )
        .unwrap();
        assert_eq!(
            accepted,
            AcceptedAssertion {
                jti: "j1".to_string(),
                exp: 160
            }
        );
    }

    #[test]
    fn rejects_wrong_party_audience_lifetime_or_missing_jti() {
        let base = serde_json::json!({
            "iss": "c1", "sub": "c1", "aud": "https://id.example.com", "exp": 160, "jti": "j1"
        });
        let with = |key: &str, value: serde_json::Value| {
            let mut claims = base.clone();
            claims[key] = value;
            claims
        };
        for bad in [
            with("iss", "c2".into()),
            with("sub", "c2".into()),
            with("aud", "https://other.example.com".into()),
            with("exp", (100 + MAX_ASSERTION_LIFETIME_SECS + 1).into()),
            with("jti", " ".into()),
        ] {
            assert!(
                accept(claims(bad.clone()), "c1", &audiences(), 100).is_err(),
                "{bad}"
            );
        }
        let mut without_jti = base.clone();
        without_jti.as_object_mut().unwrap().remove("jti");
        assert!(accept(claims(without_jti), "c1", &audiences(), 100).is_err());
    }

    #[test]
    fn reads_the_claimed_client_without_verifying() {
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"iss":"c1","sub":"c1"}"#);
        assert_eq!(
            claimed_client_id(&format!("h.{payload}.s")).as_deref(),
            Some("c1")
        );
        assert_eq!(claimed_client_id("not-a-jwt"), None);
    }
}
