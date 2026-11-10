use crate::ctx::audit_request_from;
use crate::err::{ErrorCode, ErrorResponse};
use crate::iam::auth::jwt::jwt_config;
use crate::iam::user::profile::locale as user_locale;
use crate::web::request::RequestExt;
use axum::Json;
use axum::extract::{FromRequest, Request};
use db::ent::{TrustedAuditActor, TrustedAuditContext};
use serde::{Deserialize, Serialize};
use tools::Locale;

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserSchema {
    pub id: String,
    pub email: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub nickname: String,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
    /// The saved language (`users.attrs.locale`), when one is set.
    pub locale: Option<String>,
}

impl From<db::ent::User> for UserSchema {
    fn from(user: db::ent::User) -> Self {
        let locale = user_locale(&user).map(|locale| locale.as_str().to_string());
        UserSchema {
            id: user.id,
            email: user.email,
            given_name: user.given_name,
            family_name: user.family_name,
            nickname: user.nickname,
            picture: user.picture,
            phone_number: user.phone_number,
            locale,
        }
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProfileRequest {
    pub locale: String,
}

/// Authenticate a profile request with a native session token or, for the
/// Console, an OAuth access token. Both must belong to a live session.
async fn session_user(token: String) -> Result<db::ent::User, ErrorResponse> {
    let jwt = jwt_config();
    let claims = if let Ok(claims) = jwt.validate_session_access_token(&token) {
        claims
    } else {
        let claims = jwt
            .validate_oauth_access_token(&token, None)
            .map_err(|_| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;
        let client_id = claims
            .azp
            .as_deref()
            .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;
        if !crate::iam::oidc::client_is_trusted_first_party(client_id).await {
            return Err(ErrorResponse::new(ErrorCode::AuthTokenInvalid));
        }
        claims
    };

    if crate::iam::auth::revocation::is_revoked(&claims)
        .await
        .map_err(ErrorResponse::internal)?
    {
        return Err(ErrorResponse::new(ErrorCode::AuthTokenInvalid));
    }

    let sid = claims
        .sid
        .as_deref()
        .filter(|sid| !sid.is_empty())
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;
    // By id: the email in `sub` may have changed, and a lookup by address
    // must never resolve to another account's nickname.
    let user_id = claims
        .sub_id
        .as_deref()
        .filter(|user_id| !user_id.is_empty())
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;
    if crate::iam::sess::registry::get_active_session(sid, user_id)
        .await
        .map_err(ErrorResponse::internal)?
        .is_none()
    {
        return Err(ErrorResponse::new(ErrorCode::AuthTokenInvalid));
    }
    crate::data::db::get_user_by_id(user_id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::UserNotFound))
}

#[utoipa::path(
    get,
    path = "/account/profile",
    tags = ["Account"],
    summary = "Get User Profile",
    description = "Retrieve the profile information of the currently authenticated user using their access token.",
    responses(
        (status = 200, description = "OK", body = UserSchema),
        (status = 401, description = "Unauthorized - Invalid Token", body = ErrorResponse),
        (status = 404, description = "Not Found", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn get_profile(req: Request) -> Result<Json<UserSchema>, ErrorResponse> {
    let token = req
        .get_token()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenMissing))?;
    Ok(Json(session_user(token).await?.into()))
}

#[utoipa::path(
    put,
    path = "/account/profile",
    tags = ["Account"],
    summary = "Update User Profile",
    description = "Save the authenticated user's preferred language as `users.attrs.locale`.",
    request_body = UpdateProfileRequest,
    responses(
        (status = 200, description = "OK", body = UserSchema),
        (status = 400, description = "Unsupported locale", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid Token", body = ErrorResponse),
        (status = 404, description = "Not Found", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn put_profile(req: Request) -> Result<Json<UserSchema>, ErrorResponse> {
    let audit_request = audit_request_from(req.extensions());
    let token = req
        .get_token()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenMissing))?;
    let user = session_user(token).await?;

    let Json(body) = Json::<UpdateProfileRequest>::from_request(req, &())
        .await
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::RequestInvalidJson).with_message(error.body_text())
        })?;
    let locale = Locale::parse(&body.locale).ok_or_else(|| {
        ErrorResponse::new(ErrorCode::RequestInvalid).with_param("field", "locale")
    })?;

    let audit_context =
        TrustedAuditContext::application(TrustedAuditActor::user(&user.id), audit_request);
    let user = crate::data::db::update_user_locale(&user.id, locale.as_str(), audit_context)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(user.into()))
}

#[cfg(all(test, feature = "sqlite", feature = "memory"))]
mod tests {
    use super::*;
    use crate::data::db::{service, with_test_database};
    use crate::data::store::use_store;
    use crate::iam::auth::subject::Subject;
    use crate::iam::sess::registry::{NewSession, register_session};
    use axum::{Router, body::Body, routing::get};
    use db::{
        DbStore,
        ent::{CredentialType, OAuthClient, OutboxEventFilter, Profile},
    };
    use http::StatusCode;
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use store::Store;
    use tower::ServiceExt;

    fn context() -> TrustedAuditContext {
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

    struct Fixture {
        user: db::ent::User,
        sid: String,
        client_id: String,
    }

    impl Fixture {
        async fn new(first_party: bool, enabled: bool) -> Self {
            let user = service()
                .create_user(
                    Profile::new("alice@example.test".into(), "alice".into())
                        .attrs(json!({"locale":"en", "retained":true})),
                    CredentialType::Password,
                    "unused-password-hash",
                    None,
                    context(),
                )
                .await
                .unwrap();
            let sid = ulid::Ulid::generate().to_string();
            use_store()
                .set(&sid, &Subject::from(user.clone()), Some(3600))
                .await
                .unwrap();
            register_session(
                NewSession {
                    user_id: &user.id,
                    sid: &sid,
                    org_id: None,
                    authenticated_at_unix: chrono::Utc::now().timestamp() as u64,
                    amr: vec!["pwd".into()],
                },
                "refresh-jti",
                3600,
            )
            .await
            .unwrap();
            let client_id = "stargate_console".to_string();
            let mut client = OAuthClient::new(
                client_id.clone(),
                None,
                "Client".into(),
                None,
                "none".into(),
                vec!["authorization_code".into()],
                vec!["code".into()],
                vec!["https://console.example.test/callback".into()],
                vec!["openid".into()],
                vec![],
                json!({"first_party": first_party}),
            );
            client.enabled = enabled;
            service()
                .create_oauth_client(client, context())
                .await
                .unwrap();
            Self {
                user,
                sid,
                client_id,
            }
        }

        fn claims(&self, oauth: bool) -> jwt::Claims {
            jwt::Claims {
                sub: if oauth {
                    self.user.id.clone()
                } else {
                    self.user.email.clone()
                },
                sub_id: Some(self.user.id.clone()),
                sid: Some(self.sid.clone()),
                azp: oauth.then(|| self.client_id.clone()),
                scope: oauth.then(|| "openid".into()),
                ..Default::default()
            }
        }

        async fn snapshot(&self) -> Value {
            json!({
                "user":service().get_user_by_id(&self.user.id).await.unwrap(),
                "audit":service().query_outbox_events(&OutboxEventFilter::default(),1000,0).await.unwrap().into_iter().map(|row| row.payload).collect::<Vec<_>>(),
            })
        }
    }

    async fn request(method: &str, token: &str) -> (StatusCode, Value) {
        let req = Request::builder()
            .method(method)
            .uri("/account/profile")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"));
        let response = Router::new()
            .route("/account/profile", get(get_profile).put(put_profile))
            .oneshot(
                req.body(Body::from(json!({"locale":"it"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        (status, body)
    }

    async fn assert_rejected(f: &Fixture, token: &str) {
        let before = f.snapshot().await;
        for method in ["GET", "PUT"] {
            let (status, body) = request(method, token).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{method}: {body}");
            assert_eq!(body["code"], "auth.token_invalid");
            assert_eq!(f.snapshot().await, before, "{method} changed persistence");
        }
    }

    #[tokio::test]
    async fn untrusted_clients_and_mismatched_sessions_cannot_read_or_update_profiles() {
        for (first_party, enabled, wrong_user) in [
            (false, true, false),
            (true, false, false),
            (true, true, true),
        ] {
            with_test_database(async {
                let f = Fixture::new(first_party, enabled).await;
                let mut claims = f.claims(true);
                if wrong_user {
                    claims.sub_id = Some(ulid::Ulid::generate().to_string());
                }
                let token = jwt_config().generate_oauth_access_token(claims).unwrap();
                assert_rejected(&f, &token).await;
            })
            .await;
        }
    }

    #[tokio::test]
    async fn console_and_native_sessions_can_read_and_update_profiles() {
        for oauth in [true, false] {
            with_test_database(async {
                let f = Fixture::new(true, true).await;
                let token = jwt_config()
                    .generate_session_access_token(f.claims(oauth))
                    .unwrap();
                let (status, body) = request("GET", &token).await;
                assert_eq!(status, StatusCode::OK, "{body}");
                assert_eq!(body["id"], f.user.id);
                let (status, body) = request("PUT", &token).await;
                assert_eq!(status, StatusCode::OK, "{body}");
                assert_eq!(body["locale"], "it");
                let user = service().get_user_by_id(&f.user.id).await.unwrap().unwrap();
                assert_eq!(user.attrs, json!({"locale":"it", "retained":true}));
            })
            .await;
        }
    }
}
