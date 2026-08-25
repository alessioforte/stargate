use super::AuthResponse;
use crate::act::sessions;
use crate::err::ErrorResponse;
use crate::etc;
use crate::etc::jwt::jwt_config;
use crate::fun::{build_jwt_cookie, format_name};
use axum::Json;
use axum::response::{IntoResponse, Response};
use http::header::SET_COOKIE;
use store::Store;

pub(crate) async fn issue_user_session(
    user: db::ent::User,
    auth_time: usize,
    requested_org_id: Option<&str>,
) -> Result<Response, ErrorResponse> {
    let org = sessions::resolve_org_context(&user, requested_org_id).await?;

    let mut subject = etc::sub::Subject::from(user.clone());
    subject.org_id = org.as_ref().map(|org| org.org_id.clone());
    subject.org_role = org.as_ref().map(|org| org.role.clone());

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = format_name(&given_name, &family_name);

    let sid = ulid::Ulid::generate().to_string();

    let mut claims = jwt::Claims::default()
        .subject(user.email.to_owned())
        .sub_id(user.id.to_owned())
        .name(name)
        .email(user.email.to_owned())
        .email_verified(true)
        .sid(sid.clone());

    claims.auth_time = Some(auth_time);
    claims.org_id = org.as_ref().map(|org| org.org_id.clone());

    let is_super_admin = crate::fun::is_super_admin_user_id(&user.id)
        .await
        .map_err(ErrorResponse::internal)?;

    if is_super_admin {
        claims = claims.role(crate::fun::SUPER_ADMIN_ROLE.to_string());
    }

    let tokens = crate::fun::generate_tokens(claims).map_err(ErrorResponse::internal)?;

    let store = etc::store::use_store();
    let refresh_exp = jwt_config().refresh_exp;
    let access_exp = jwt_config().access_exp;
    let sttl: u64 = refresh_exp.as_seconds_f64() as u64;
    let cttl: i64 = access_exp.as_seconds_f64() as i64;

    store
        .set(&sid, &subject, Some(sttl))
        .await
        .map_err(ErrorResponse::internal)?;
    if let Err(error) = sessions::register_session(
        &user.id,
        &sid,
        subject.org_id.as_deref(),
        auth_time as u64,
        &tokens.refresh_jti,
        sttl,
    )
    .await
    {
        let _ = store.delete(&sid).await;
        return Err(ErrorResponse::internal(error));
    }

    let cookie = build_jwt_cookie(&tokens.access_token, cttl);
    let body = AuthResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        token_type: "Bearer".to_string(),
        org_id: subject.org_id.clone(),
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
