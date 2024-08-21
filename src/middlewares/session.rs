use actix_session::{storage::CookieSessionStore, SessionMiddleware};

pub fn cookie_session(secret_key: actix_web::cookie::Key) -> SessionMiddleware<CookieSessionStore> {
    SessionMiddleware::builder(CookieSessionStore::default(), secret_key)
        .cookie_name(String::from("token")) // arbitrary name
        .cookie_secure(false) // https only
        // .session_lifecycle(BrowserSession::default()) // expire at end of session
        // .cookie_same_site(SameSite::Strict)
        // .cookie_content_security(CookieContentSecurity::Private) // encrypt
        .cookie_http_only(true) // disallow scripts from reading
        .build()
}
