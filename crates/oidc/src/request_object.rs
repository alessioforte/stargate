//! Request objects (OIDC Core 6) are not supported, but their rejection must
//! still reach the client the way the object asked: its `response_mode` and
//! `state` supersede the query parameters (OIDC Core 6.3.3). They are read
//! without verifying the object; the `redirect_uri` they apply to is the
//! verified one from the query.

use base64::Engine;

/// Response parameters a request object carries.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ResponseParameters {
    pub response_mode: Option<String>,
    pub state: Option<String>,
}

/// The response parameters of `request` (a JWS or an unsigned JWT); none when
/// it is not a readable JWT.
pub fn response_parameters(request: &str) -> ResponseParameters {
    let Some(claims) = unverified_claims(request) else {
        return ResponseParameters::default();
    };
    let string = |name: &str| {
        claims
            .get(name)
            .and_then(|value| value.as_str())
            .map(str::to_string)
    };
    ResponseParameters {
        response_mode: string("response_mode"),
        state: string("state"),
    }
}

/// The claims of a JWT, read without checking its signature.
pub(crate) fn unverified_claims(token: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unsigned(claims: &str) -> String {
        let encode = |part: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(part);
        format!("{}.{}.", encode(r#"{"alg":"none"}"#), encode(claims))
    }

    #[test]
    fn reads_the_response_parameters() {
        assert_eq!(
            response_parameters(&unsigned(
                r#"{"response_mode":"form_post","state":"s1","scope":"openid"}"#
            )),
            ResponseParameters {
                response_mode: Some("form_post".to_string()),
                state: Some("s1".to_string()),
            }
        );
        assert_eq!(
            response_parameters(&unsigned(r#"{"scope":"openid"}"#)),
            ResponseParameters::default()
        );
        assert_eq!(
            response_parameters("not-a-jwt"),
            ResponseParameters::default()
        );
    }
}
