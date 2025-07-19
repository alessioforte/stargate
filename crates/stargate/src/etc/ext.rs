use actix_web::{http::header::Header, HttpRequest};
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};

pub trait RequestExt {
    fn get_token(&self) -> String;
}

impl RequestExt for HttpRequest {
    fn get_token(&self) -> String {
        let token = match Authorization::<Bearer>::parse(self) {
            Ok(auth) => auth.into_scheme().token().to_string(),
            Err(_) => "".to_string(),
        };

        token
    }
}
