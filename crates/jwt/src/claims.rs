use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::env;

pub fn issuer_from_env() -> String {
    env::var("JWT_ISSUER").unwrap_or_else(|_| {
        let port = env::var("PORT").unwrap_or_else(|_| "5050".to_string());
        format!("http://localhost:{port}")
    })
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub iat: usize,                         // issued at
    pub iss: String,                        // issuer
    pub exp: usize,                         // expiration
    pub sub: String,                        // subject
    pub sub_id: Option<String>,             // subject id
    pub email: Option<String>,              // email
    pub name: Option<String>,               // name
    pub email_verified: Option<bool>,       // email_verified
    pub nickname: Option<String>,           // nickname
    pub jti: Option<String>,                // JWT ID
    pub aud: Option<String>,                // audience
    pub typ: Option<String>,                // type
    pub azp: Option<String>,                // authorized party
    pub sid: Option<String>,                // session ID
    pub scope: Option<String>,              // scope
    pub role: Option<String>,               // role
    pub auth_time: Option<usize>,           // authentication time
    pub nonce: Option<String>,              // OIDC nonce
    pub preferred_username: Option<String>, // OIDC preferred username
    pub picture: Option<String>,            // OIDC profile picture
}

impl Default for Claims {
    fn default() -> Self {
        let now = Utc::now();
        let iss = issuer_from_env();

        Claims {
            iss,
            iat: now.timestamp() as usize,
            exp: (now + Duration::minutes(60)).timestamp() as usize,
            sub: "".to_string(),
            email: None,
            sub_id: None,
            name: None,
            email_verified: None,
            nickname: None,
            jti: None,
            aud: None,
            typ: None,
            azp: None,
            sid: None,
            scope: None,
            role: None,
            auth_time: None,
            nonce: None,
            preferred_username: None,
            picture: None,
        }
    }
}

impl Claims {
    pub fn iat(mut self, iat: usize) -> Self {
        self.iat = iat;
        self
    }

    pub fn exp(mut self, exp: usize) -> Self {
        self.exp = exp;
        self
    }

    pub fn iss(mut self, iss: String) -> Self {
        self.iss = iss;
        self
    }

    pub fn subject(mut self, sub: String) -> Self {
        self.sub = sub;
        self
    }

    pub fn sub_id(mut self, sub_id: String) -> Self {
        self.sub_id = Some(sub_id);
        self
    }

    pub fn email(mut self, email: String) -> Self {
        self.email = Some(email);
        self
    }

    pub fn name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn email_verified(mut self, email_verified: bool) -> Self {
        self.email_verified = Some(email_verified);
        self
    }

    pub fn nickname(mut self, nickname: String) -> Self {
        self.nickname = Some(nickname);
        self
    }

    pub fn sid(mut self, sid: String) -> Self {
        self.sid = Some(sid);
        self
    }

    pub fn role(mut self, role: String) -> Self {
        self.role = Some(role);
        self
    }

    pub fn jti(mut self, jti: String) -> Self {
        self.jti = Some(jti);
        self
    }

    pub fn aud(mut self, aud: String) -> Self {
        self.aud = Some(aud);
        self
    }

    pub fn typ(mut self, typ: String) -> Self {
        self.typ = Some(typ);
        self
    }
}
