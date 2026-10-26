use super::AuthResponse;
use crate::err::ErrorResponse;
use crate::iam::sess::issuance::{IssuedSession, start_user_session};
use crate::web::cookies::build_jwt_cookie;
use axum::Json;
use axum::response::{IntoResponse, Response};
use http::header::SET_COOKIE;

/// The session cookie that carries the access token.
pub(crate) fn session_cookie(session: &IssuedSession) -> String {
    build_jwt_cookie(&session.access_token, session.access_ttl_secs)
}

/// The tokens as JSON, with the session cookie.
pub(crate) fn session_response(session: IssuedSession) -> Result<Response, ErrorResponse> {
    let cookie = session_cookie(&session);
    let body = AuthResponse {
        access_token: session.access_token,
        refresh_token: session.refresh_token,
        token_type: "Bearer".to_string(),
        org_id: session.org_id,
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

/// Sign `user` in and answer with the tokens; see [`start_user_session`].
pub(crate) async fn issue_user_session(
    user: db::ent::User,
    auth_time: usize,
    requested_org_id: Option<&str>,
    amr: Vec<String>,
) -> Result<Response, ErrorResponse> {
    session_response(start_user_session(user, auth_time, requested_org_id, amr).await?)
}
