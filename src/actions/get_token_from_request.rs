use actix_session::Session;
use actix_web::{http::header::Header, FromRequest, HttpRequest};
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};

pub fn get_token_from_request(req: &HttpRequest) -> String {
    let mut token = match Authorization::<Bearer>::parse(req) {
        Ok(auth) => auth.into_scheme().token().to_string(),
        Err(_) => "".to_string(),
    };

    if token.is_empty() {
        let session = Session::extract(req).into_inner().ok();
        token = match session.and_then(|s| s.get::<String>("token").ok().flatten()) {
            Some(token) => token,
            None => "".to_string(),
        };
    }

    token
}
