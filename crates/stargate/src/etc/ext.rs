use actix_web::{HttpRequest, http::header::Header, web::Query};
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};

const KEYS: &[&str] = &["token", "access_token", "jwt"];

pub trait RequestExt {
    fn get_token(&self) -> String;
    fn get_api_key(&self) -> String;
}

impl RequestExt for HttpRequest {
    fn get_token(&self) -> String {
        // get token from Authorization header
        let mut token = match Authorization::<Bearer>::parse(self) {
            Ok(auth) => auth.into_scheme().token().to_string(),
            Err(_) => "".to_string(),
        };

        // get token from cookie
        if token.is_empty() {
            if let Some(cookie) = self.cookie("jwt") {
                token = cookie.value().to_string();
            }
        }

        // get token from query string
        if token.is_empty() {
            let query = self.query_string();
            let entries = Query::<std::collections::HashMap<String, String>>::from_query(query);
            if let Ok(entries) = entries {
                for key in KEYS {
                    if let Some(value) = entries.get(*key) {
                        token = value.to_string();
                        break;
                    }
                }
            }
        }

        token
    }

    fn get_api_key(&self) -> String {
        let api_key = match self.headers().get("x-api-key") {
            Some(header_value) => header_value.to_str().unwrap_or("").to_string(),
            None => "".to_string(),
        };

        println!("API Key: {}", api_key);
        api_key
    }
}
