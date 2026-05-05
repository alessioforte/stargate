use super::{AuthResponse, UserCredentials, build_jwt_cookie};
use crate::act::login_guard;
use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use crate::etc::ext::RequestExt;
use crate::etc::jwt::jwt_config;
use crate::etc::reqctx::take_audit_context_from;
use crate::fun::format_name;
use axum::Json;
use axum::extract::{FromRequest, Request};
use axum::response::{IntoResponse, Response};
use db::ent::CredentialType;
use http::header::SET_COOKIE;
use pw::Hash;
use store::Store;

#[utoipa::path(
    post,
    path = "/account/login",
    tags = ["Account"],
    summary = "User Login",
    description = "Authenticate a user using their username and password. On successful authentication, an access token and a refresh token are issued.",
    request_body = UserCredentials,
    responses(
        (status = 200, description = "OK", body = AuthResponse),
        (status = 429, description = "Too Many Requests", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid Credentials", body = ErrorResponse),
        (status = 503, description = "Service Unavailable", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn post_login(mut req: Request) -> Result<Response, ErrorResponse> {
    let mut audit_ctx = take_audit_context_from(req.extensions_mut());
    let client_ip = req.get_client_ip();

    let Json(credentials) = Json::<UserCredentials>::from_request(req, &())
        .await
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.body_text())))?;

    let login_identifier = credentials.username.trim();

    let user = crate::db::get_user_by_username(login_identifier)
        .await
        .map_err(ErrorResponse::internal)?;

    let throttle = match user.as_ref() {
        Some(u) => login_guard::LoginThrottle::for_user(&client_ip, &u.id),
        None => login_guard::LoginThrottle::for_identifier(&client_ip, login_identifier),
    };

    let allowed = login_guard::check_login_allowed(&throttle)
        .await
        .map_err(login_guard_unavailable)?;
    if !allowed {
        let mut err = ErrorResponse::from(HttpError::TooManyRequests(
            "too many failed login attempts, try again later".to_string(),
        ));
        err.insert_header("Retry-After", &login_guard::lockout_seconds().to_string());
        return Err(err);
    }

    let Some(user) = user else {
        login_guard::record_failed_attempt(&throttle)
            .await
            .map_err(login_guard_unavailable)?;
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid credentials".to_string(),
        )));
    };

    let user_credential = crate::db::get_credential(&user.id, CredentialType::Password)
        .await
        .map_err(ErrorResponse::internal)?;

    let Some(user_credential) = user_credential else {
        login_guard::record_failed_attempt(&throttle)
            .await
            .map_err(login_guard_unavailable)?;
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid credentials".to_string(),
        )));
    };

    if Hash::verify(&credentials.password, &user_credential.value).is_err() {
        login_guard::record_failed_attempt(&throttle)
            .await
            .map_err(login_guard_unavailable)?;
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid credentials".to_string(),
        )));
    }

    if let Err(error) = login_guard::clear_subject_attempts(&throttle).await {
        tracing::warn!(
            "Failed to clear login attempts after successful login: {}",
            error
        );
    }

    audit_ctx = audit_ctx.with_actor(db::ent::ActorType::User, Some(user.id.clone()));
    // Note: no downstream consumer in the handler chain (Stage D wires middleware);
    // drop audit_ctx after enrichment.
    let _ = audit_ctx;

    let subject = etc::sub::Subject::from(user.clone());

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = format_name(&given_name, &family_name);

    let sid = ulid::Ulid::new().to_string();
    let mut claims = jwt::Claims::default()
        .subject(user.email.to_owned())
        .sub_id(user.id.to_owned())
        .name(name)
        .email(user.email.to_owned())
        .email_verified(true)
        .sid(sid.clone());

    let is_super_admin = crate::fun::is_super_admin_user_id(&user.id)
        .await
        .map_err(ErrorResponse::internal)?;
    if is_super_admin {
        claims = claims.role(crate::fun::SUPER_ADMIN_ROLE.to_string());
    }

    let (access_token, refresh_token) =
        crate::fun::generate_tokens(claims).map_err(ErrorResponse::internal)?;

    let store = etc::store::use_store();
    let refresh_exp = jwt_config().refresh_exp;
    let access_exp = jwt_config().access_exp;
    let sttl: u64 = refresh_exp.as_seconds_f64() as u64;
    let cttl: i64 = access_exp.as_seconds_f64() as i64;

    store
        .set(&sid, &subject, Some(sttl))
        .await
        .map_err(ErrorResponse::internal)?;

    let cookie = build_jwt_cookie(&access_token, cttl);
    let body = AuthResponse {
        access_token,
        refresh_token,
        token_type: "Bearer".to_string(),
    };

    let mut resp = Json(body).into_response();
    resp.headers_mut().insert(
        SET_COOKIE,
        cookie
            .parse()
            .map_err(|_| ErrorResponse::internal("invalid cookie header"))?,
    );
    Ok(resp)
}

fn login_guard_unavailable(error: store::StoreError) -> ErrorResponse {
    tracing::error!("Login guard unavailable: {}", error);
    ErrorResponse::from(HttpError::ServiceUnavailable(
        "login temporarily unavailable".to_string(),
    ))
}
