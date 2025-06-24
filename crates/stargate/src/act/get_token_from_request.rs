use actix_web::{http::header::Header, HttpRequest};
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};

pub fn get_token_from_request(req: &HttpRequest) -> String {
    let token = match Authorization::<Bearer>::parse(req) {
        Ok(auth) => auth.into_scheme().token().to_string(),
        Err(_) => "".to_string(),
    };

    token
}
