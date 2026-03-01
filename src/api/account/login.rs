use super::{AuthResponse, UserCredentials};
use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use crate::etc::jwt::jwt_config;
use crate::fun::format_name;
use actix_web::{HttpMessage, HttpRequest, HttpResponse, cookie::Cookie, post, web};
use db::ent::AuditContext;
use db::ent::CredentialType;
use pw::Hash;
use store::Store;

#[utoipa::path(
    context_path = "/account",
    path = "/login",
    tags = ["Account"],
    summary = "User Login",
    description = "Authenticate a user using their username and password. On successful authentication, an access token and a refresh token are issued.",
    responses(
        (status = 200, description = "OK", body = AuthResponse),
        (status = 401, description = "Unauthorized - Invalid Credentials", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
#[post("/login")]
pub async fn handler(
    req: HttpRequest,
    credentials: web::Json<UserCredentials>,
) -> Result<HttpResponse, ErrorResponse> {
    let user = match crate::db::get_user_by_username(&credentials.username).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    if user.is_none() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid credentials".to_string(),
        )));
    }

    let user = user.unwrap();
    let user_credential = match crate::db::get_credential(&user.id, CredentialType::Password).await
    {
        Ok(credential) => credential,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    let password = match user_credential {
        Some(c) => c.value,
        None => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid credentials".to_string(),
            )));
        }
    };

    if Hash::verify(&credentials.password, &password).is_err() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid credentials".to_string(),
        )));
    }

    let mut ctx = req
        .extensions_mut()
        .remove::<AuditContext>()
        .unwrap_or_else(AuditContext::anonymous);
    ctx = ctx.with_actor(db::ent::ActorType::User, Some(user.id.clone()));
    req.extensions_mut().insert(ctx);

    let subject = etc::sub::Subject::from(user.clone());

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = format_name(&given_name, &family_name);

    let sid = ulid::Ulid::new().to_string();
    let mut claims = jwt::Claims::default()
        .subject(user.email.to_owned())
        .sub_id(user.id.to_owned())
        .name(name.clone())
        .email(user.email.to_owned())
        .email_verified(true)
        .sid(sid.clone());

    if user.nickname == crate::etc::consts::STARGATE_ADMIN {
        claims = claims.role(crate::etc::consts::STARGATE_ADMIN.to_string());
    }

    let (access_token, refresh_token) = match crate::fun::generate_tokens(claims) {
        Ok(tokens) => tokens,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    // Store the user ID in the session
    let store = etc::store::use_store();
    let refresh_exp = jwt_config().refresh_exp;
    let access_exp = jwt_config().access_exp;
    let sttl: u64 = refresh_exp.as_seconds_f64() as u64;
    let cttl: i64 = access_exp.as_seconds_f64() as i64;

    match store.set(&sid, &subject, Some(sttl)).await {
        Ok(_) => {}
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    }

    let cookie = Cookie::build("jwt", access_token.clone())
        .path("/")
        .http_only(true)
        .same_site(actix_web::cookie::SameSite::Lax)
        .max_age(actix_web::cookie::time::Duration::seconds(cttl))
        .finish();

    Ok(HttpResponse::Ok()
        .cookie(cookie)
        .json(web::Json(AuthResponse {
            access_token,
            refresh_token,
            token_type: "Bearer".to_string(),
        })))
}
