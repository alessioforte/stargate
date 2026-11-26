//! The Console and the admin API on different origins: OAuth stays on the
//! issuer (where the session cookie lives) and the Console calls `/admin` on
//! its own origin with the bearer token it received.

use super::*;
use ::db::DbStore;
use ::db::ent::OAuthClient;

const CONSOLE_CLIENT_ID: &str = "console-flow";
const CONSOLE_ORIGIN: &str = "https://console.example.com";
const CONSOLE_REDIRECT_URI: &str = "https://console.example.com/stargate/auth/callback";
const CONSOLE_POST_LOGOUT_URI: &str = "https://console.example.com/stargate/";
const CODE_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

async fn console_client() {
    let mut client = OAuthClient::new(
        CONSOLE_CLIENT_ID.into(),
        None,
        "Console".into(),
        None,
        "none".into(),
        vec!["authorization_code".into(), "refresh_token".into()],
        vec!["code".into()],
        vec![CONSOLE_REDIRECT_URI.into()],
        vec![
            "openid".into(),
            "email".into(),
            "profile".into(),
            "offline_access".into(),
        ],
        Vec::new(),
        json!({"first_party": true}),
    );
    client.post_logout_redirect_uris.0 = vec![CONSOLE_POST_LOGOUT_URI.into()];
    db::create_oauth_client(client, context()).await.unwrap();
}

fn form_request(path: &str, fields: &[(&str, &str)]) -> Request<Body> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields.iter().copied())
        .finish();
    Request::post(path)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::ORIGIN, CONSOLE_ORIGIN)
        .body(Body::from(body))
        .unwrap()
}

fn authorize_path(state: &str, prompt: Option<&str>) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("response_type", "code")
        .append_pair("client_id", CONSOLE_CLIENT_ID)
        .append_pair("redirect_uri", CONSOLE_REDIRECT_URI)
        .append_pair("scope", "openid email profile offline_access")
        .append_pair("state", state)
        .append_pair("code_challenge", &oidc::pkce::s256_challenge(CODE_VERIFIER))
        .append_pair("code_challenge_method", "S256");
    if let Some(prompt) = prompt {
        query.append_pair("prompt", prompt);
    }
    format!("/oauth/authorize?{}", query.finish())
}

/// A browser navigation from the Console carries no `Origin`, but Fetch
/// Metadata marks it cross-site; `/oauth/authorize` and `/oauth/logout` stay
/// browser entry points for that.
fn navigation(path: &str, cookie: &str) -> Request<Body> {
    Request::get(path)
        .header(header::COOKIE, format!("jwt={cookie}"))
        .header("sec-fetch-site", "cross-site")
        .body(Body::empty())
        .unwrap()
}

fn redirect_location(response: axum::response::Response) -> String {
    response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string()
}

fn query_param(location: &str, name: &str) -> String {
    let url = url::Url::parse(location).unwrap();
    url.query_pairs()
        .find(|(key, _)| key == name)
        .unwrap_or_else(|| panic!("missing {name} in {location}"))
        .1
        .into_owned()
}

fn encoded(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

pub(super) async fn run(router: &Router) {
    console_client().await;

    let name = ulid::Ulid::generate().to_string();
    let user = db::service()
        .create_super_admin_user(
            Profile::new(format!("{name}@example.test"), name),
            CredentialType::Password,
            "fixture-password-hash",
            TrustedAuditContext::system_control_plane(),
        )
        .await
        .unwrap();
    let (sid, native, _, _) = start_session(&user, None).await;

    // The Console opens the provider at the issuer: the browser's session
    // cookie authorizes the code, which goes back to the Console origin.
    let response = router
        .clone()
        .oneshot(navigation(&authorize_path("console-state", None), &native))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FOUND);
    let location = redirect_location(response);
    assert_eq!(location.split('?').next(), Some(CONSOLE_REDIRECT_URI));
    assert_eq!(query_param(&location, "state"), "console-state");
    let code = query_param(&location, "code");

    // The Console exchanges the code and refreshes from its own origin.
    let tokens = send(
        router,
        form_request(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", CONSOLE_CLIENT_ID),
                ("redirect_uri", CONSOLE_REDIRECT_URI),
                ("code", &code),
                ("code_verifier", CODE_VERIFIER),
            ],
        ),
        StatusCode::OK,
    )
    .await;
    let access_token = tokens["access_token"].as_str().unwrap().to_string();
    let refresh_token = tokens["refresh_token"].as_str().unwrap().to_string();
    let id_token = tokens["id_token"].as_str().unwrap().to_string();

    let refreshed = send(
        router,
        form_request(
            "/oauth/token",
            &[
                ("grant_type", "refresh_token"),
                ("client_id", CONSOLE_CLIENT_ID),
                ("refresh_token", &refresh_token),
            ],
        ),
        StatusCode::OK,
    )
    .await;
    assert!(refreshed["access_token"].is_string());

    // The admin API answers the Console on its own origin with the bearer.
    let admin = send(
        router,
        Request::get("/admin/me")
            .header(header::AUTHORIZATION, bearer(&access_token))
            .header(header::ORIGIN, CONSOLE_ORIGIN)
            .body(Body::empty())
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(admin["user"]["id"], user.id);

    // RP-Initiated Logout from the Console closes the issuer session and
    // returns to the Console root.
    let logout_path = format!(
        "/oauth/logout?id_token_hint={}&client_id={CONSOLE_CLIENT_ID}&post_logout_redirect_uri={}&state=console-state",
        encoded(&id_token),
        encoded(CONSOLE_POST_LOGOUT_URI),
    );
    let response = router
        .clone()
        .oneshot(navigation(&logout_path, &native))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        redirect_location(response),
        format!("{CONSOLE_POST_LOGOUT_URI}?state=console-state")
    );

    // The same browser cookie no longer authorizes: the issuer session is
    // gone, so the provider asks for a sign-in again.
    let response = router
        .clone()
        .oneshot(navigation(
            &authorize_path("after-logout", Some("none")),
            &native,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FOUND);
    let location = redirect_location(response);
    assert_eq!(query_param(&location, "error"), "login_required");

    registry::revoke_session(&sid).await.unwrap();
}
