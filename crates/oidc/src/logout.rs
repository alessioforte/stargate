//! OpenID Connect RP-Initiated Logout 1.0 and Back-Channel Logout 1.0.

use serde::Serialize;
use serde_json::{Map, Value};

/// `events` member that identifies a Back-Channel Logout token (§2.4).
pub const BACKCHANNEL_LOGOUT_EVENT: &str = "http://schemas.openid.net/event/backchannel-logout";

/// Logout tokens are delivered immediately; a short lifetime limits replay.
pub const LOGOUT_TOKEN_TTL_SECS: i64 = 120;

/// Claims of a Back-Channel Logout token (§2.4): the ID token's `iss`,
/// `sub`, and `aud`, the session's `sid`, and the logout event. A logout
/// token never carries a `nonce`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogoutTokenClaims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
    pub events: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,
}

pub fn logout_token_claims(
    issuer: &str,
    subject: &str,
    client_id: &str,
    sid: Option<&str>,
    now_unix: i64,
) -> LogoutTokenClaims {
    let mut events = Map::new();
    events.insert(
        BACKCHANNEL_LOGOUT_EVENT.to_string(),
        Value::Object(Map::new()),
    );
    LogoutTokenClaims {
        iss: issuer.to_string(),
        sub: subject.to_string(),
        aud: client_id.to_string(),
        iat: now_unix,
        exp: now_unix + LOGOUT_TOKEN_TTL_SECS,
        jti: ulid::Ulid::generate().to_string(),
        events,
        sid: sid.map(str::to_string),
    }
}

/// `post_logout_redirect_uri` must exactly match a registered value
/// (RP-Initiated Logout 1.0 §3).
pub fn post_logout_redirect_registered(registered: &[String], uri: &str) -> bool {
    registered.iter().any(|value| value == uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logout_token_claims_carry_the_event_and_no_nonce() {
        let claims = logout_token_claims("https://op", "user-1", "client-1", Some("sid-1"), 100);
        let json = serde_json::to_value(&claims).unwrap();

        assert_eq!(json["aud"], "client-1");
        assert_eq!(json["sid"], "sid-1");
        assert_eq!(json["exp"], 100 + LOGOUT_TOKEN_TTL_SECS);
        assert_eq!(
            json["events"][BACKCHANNEL_LOGOUT_EVENT],
            serde_json::json!({})
        );
        assert!(json.get("nonce").is_none());
    }

    #[test]
    fn post_logout_redirect_requires_an_exact_match() {
        let registered = vec!["https://app.example/logged-out".to_string()];

        assert!(post_logout_redirect_registered(
            &registered,
            "https://app.example/logged-out"
        ));
        assert!(!post_logout_redirect_registered(
            &registered,
            "https://app.example/logged-out/"
        ));
        assert!(!post_logout_redirect_registered(
            &registered,
            "https://app.example/logged-out?x=1"
        ));
        assert!(!post_logout_redirect_registered(
            &[],
            "https://app.example/logged-out"
        ));
    }
}
