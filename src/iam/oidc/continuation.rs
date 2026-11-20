//! Login initiated by a third party (OpenID Connect Core 1.0 §4).
//!
//! An app that wants the user back after a flow Stargate starts (an
//! invitation, a self-registration, a password reset) registers an
//! `initiate_login_uri`. When such a flow begins inside a parked authorization
//! request, the pending request records its client; the completion page then
//! sends the browser to that endpoint with `iss` and `login_hint`, and the app
//! starts a normal authorization request from there.

use super::{client_url, interactions};
use crate::err::ErrorResponse;
use serde::{Deserialize, Serialize};

/// The app a flow began in, kept with the pending request so a link opened
/// later in another tab still ends in that app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginContinuation {
    pub client_id: String,
    /// `target_link_uri`: where the app should land after the sign-in.
    #[serde(default)]
    pub target_link_uri: Option<String>,
}

impl LoginContinuation {
    pub fn new(client_id: impl Into<String>, target_link_uri: Option<String>) -> Self {
        Self {
            client_id: client_id.into(),
            target_link_uri,
        }
    }
}

/// Where to send the browser once the flow completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Continuation {
    pub url: String,
    pub client_name: String,
}

/// The app that parked `interaction_id`. An absent or expired interaction, or
/// a client that is gone, disabled, or has no `initiate_login_uri`, yields
/// none: the flow simply ends on Stargate's pages.
pub async fn from_interaction(
    interaction_id: Option<&str>,
) -> Result<Option<LoginContinuation>, ErrorResponse> {
    let Some(interaction_id) = interaction_id else {
        return Ok(None);
    };
    let Some(pending) = interactions::get(interaction_id).await? else {
        return Ok(None);
    };
    let Some(client_id) = pending.query.client_id else {
        return Ok(None);
    };
    let Some(client) = crate::data::db::get_oauth_client_by_client_id(&client_id)
        .await
        .map_err(ErrorResponse::internal)?
    else {
        return Ok(None);
    };
    Ok(can_continue(&client).then(|| LoginContinuation::new(client_id, None)))
}

/// The endpoint to send the user to, and the app's name. The client is read
/// again: it may have been disabled or changed since the flow started.
pub async fn complete(
    continuation: Option<&LoginContinuation>,
    email: &str,
) -> anyhow::Result<Option<Continuation>> {
    let Some(continuation) = continuation else {
        return Ok(None);
    };
    let Some(client) =
        crate::data::db::get_oauth_client_by_client_id(&continuation.client_id).await?
    else {
        return Ok(None);
    };
    if !can_continue(&client) {
        return Ok(None);
    }
    let Some(uri) = client.initiate_login_uri.as_deref() else {
        return Ok(None);
    };
    let url = initiate_login_url(
        &continuation.client_id,
        uri,
        &jwt::issuer_from_env(),
        email,
        continuation.target_link_uri.as_deref(),
    );
    Ok(url.map(|url| Continuation {
        url,
        client_name: client.name,
    }))
}

fn can_continue(client: &db::ent::OAuthClient) -> bool {
    client.enabled && client.initiate_login_uri.is_some()
}

/// `initiate_login_uri` with `iss`, `login_hint`, and, when set,
/// `target_link_uri` (OpenID Connect Core 1.0 §4). Query parameters the app
/// already put in the endpoint are kept.
///
/// The endpoint is validated again here: registration rejects anything that
/// is not a client endpoint URL, but a row can also reach the database from an
/// import or direct SQL, and the page uses the result as an `href`. The value
/// is never logged: a rejected scheme is exactly what must not be followed.
pub fn initiate_login_url(
    client_id: &str,
    base: &str,
    issuer: &str,
    email: &str,
    target_link_uri: Option<&str>,
) -> Option<String> {
    if !client_url::is_client_endpoint_url(base) {
        tracing::warn!(
            client_id,
            "OAuth client initiate_login_uri is not a client endpoint URL; no continuation"
        );
        return None;
    }
    let mut url = url::Url::parse(base).ok()?;
    let mut query = url.query_pairs_mut();
    query
        .append_pair("iss", issuer)
        .append_pair("login_hint", email);
    if let Some(target_link_uri) = target_link_uri {
        query.append_pair("target_link_uri", target_link_uri);
    }
    drop(query);
    Some(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::initiate_login_url;

    #[test]
    fn initiate_login_url_carries_the_issuer_and_the_hinted_email() {
        assert_eq!(
            initiate_login_url(
                "app",
                "https://app.example/login",
                "https://id.example",
                "ada@example.com",
                None
            )
            .unwrap(),
            "https://app.example/login?iss=https%3A%2F%2Fid.example&login_hint=ada%40example.com"
        );
    }

    #[test]
    fn initiate_login_url_keeps_existing_parameters_and_adds_the_target() {
        assert_eq!(
            initiate_login_url(
                "app",
                "https://app.example/login?tenant=acme",
                "https://id.example",
                "ada@example.com",
                Some("https://app.example/inbox?a=1"),
            )
            .unwrap(),
            "https://app.example/login?tenant=acme&iss=https%3A%2F%2Fid.example&login_hint=ada%40example.com&target_link_uri=https%3A%2F%2Fapp.example%2Finbox%3Fa%3D1"
        );
    }

    #[test]
    fn an_unparsable_endpoint_has_no_continuation() {
        assert_eq!(
            initiate_login_url("app", "not a url", "https://id.example", "a@b.c", None),
            None
        );
    }

    #[test]
    fn an_endpoint_that_is_not_a_client_url_has_no_continuation() {
        for base in [
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "http://app.example/login",
            "https://app.example/login#fragment",
        ] {
            assert_eq!(
                initiate_login_url("app", base, "https://id.example", "a@b.c", None),
                None,
                "{base}"
            );
        }
        assert!(
            initiate_login_url(
                "app",
                "http://localhost:3000/login",
                "https://id.example",
                "a@b.c",
                None,
            )
            .is_some()
        );
    }
}

#[cfg(all(test, feature = "memory", feature = "sqlite"))]
mod store_tests {
    use super::super::interactions;
    use super::super::request::AuthorizationRequest;
    use super::{Continuation, LoginContinuation, complete, from_interaction};
    use crate::data::db::with_test_database;
    use db::ent::{OAuthClient, TrustedAuditContext};

    fn admin_context() -> TrustedAuditContext {
        TrustedAuditContext::admin_control_plane(
            db::ent::TrustedAdminActor::admin("01JZ000000000000000000000A"),
            db::ent::TrustedAuditRequest::from_http(
                ulid::Ulid::generate().to_string(),
                None,
                None,
                None,
            ),
        )
    }

    async fn client(client_id: &str, enabled: bool, initiate_login_uri: Option<&str>) {
        let mut client = OAuthClient::new(
            client_id.to_string(),
            None,
            "Orders".to_string(),
            None,
            "none".to_string(),
            vec!["authorization_code".to_string()],
            vec!["code".to_string()],
            vec!["https://app.example/callback".to_string()],
            vec!["openid".to_string()],
            Vec::new(),
            serde_json::json!({}),
        );
        client.enabled = enabled;
        client.initiate_login_uri = initiate_login_uri.map(str::to_string);
        crate::data::db::create_oauth_client(client, admin_context())
            .await
            .expect("create client");
    }

    async fn park(client_id: &str) -> String {
        let query = AuthorizationRequest {
            client_id: Some(client_id.to_string()),
            ..AuthorizationRequest::default()
        };
        interactions::park(query).await.expect("park request")
    }

    #[tokio::test]
    async fn the_parked_client_carries_the_continuation() {
        with_test_database(async {
            client("app", true, Some("https://app.example/login")).await;
            let id = park("app").await;

            assert_eq!(
                from_interaction(Some(&id)).await.unwrap(),
                Some(LoginContinuation::new("app", None))
            );
        })
        .await;
    }

    #[tokio::test]
    async fn a_missing_or_expired_interaction_has_no_continuation() {
        with_test_database(async {
            assert_eq!(from_interaction(None).await.unwrap(), None);
            assert_eq!(from_interaction(Some("expired")).await.unwrap(), None);
        })
        .await;
    }

    #[tokio::test]
    async fn a_client_without_an_endpoint_or_disabled_has_no_continuation() {
        with_test_database(async {
            client("no-endpoint", true, None).await;
            client("disabled", false, Some("https://app.example/login")).await;

            for client_id in ["no-endpoint", "disabled"] {
                let id = park(client_id).await;
                assert_eq!(
                    from_interaction(Some(&id)).await.unwrap(),
                    None,
                    "{client_id}"
                );
            }
        })
        .await;
    }

    #[tokio::test]
    async fn completion_builds_the_endpoint_and_follows_the_client_state() {
        with_test_database(async {
            client("app", true, Some("https://app.example/login?tenant=acme")).await;
            let continuation =
                LoginContinuation::new("app", Some("https://app.example/inbox".into()));

            let completed: Continuation = complete(Some(&continuation), "ada@example.com")
                .await
                .unwrap()
                .expect("continuation");
            assert_eq!(completed.client_name, "Orders");
            let url = url::Url::parse(&completed.url).expect("absolute endpoint");
            let params = url
                .query_pairs()
                .collect::<std::collections::HashMap<_, _>>();
            assert_eq!(
                params.get("tenant").map(|value| value.as_ref()),
                Some("acme")
            );
            assert_eq!(
                params.get("iss").map(|value| value.as_ref()),
                Some(jwt::issuer_from_env().as_str())
            );
            assert_eq!(
                params.get("login_hint").map(|value| value.as_ref()),
                Some("ada@example.com")
            );
            assert_eq!(
                params.get("target_link_uri").map(|value| value.as_ref()),
                Some("https://app.example/inbox")
            );

            crate::data::db::set_oauth_client_enabled("app", false, admin_context())
                .await
                .expect("disable client");
            assert_eq!(
                complete(Some(&continuation), "ada@example.com")
                    .await
                    .unwrap(),
                None
            );
            assert_eq!(complete(None, "ada@example.com").await.unwrap(), None);
        })
        .await;
    }
}
