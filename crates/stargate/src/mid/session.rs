use actix_session::{storage::CookieSessionStore, SessionMiddleware};

// TODO: Verify how it works and if it necessary to use `actix-session`
pub fn cookie_session(secret_key: actix_web::cookie::Key) -> SessionMiddleware<CookieSessionStore> {
    SessionMiddleware::builder(CookieSessionStore::default(), secret_key).build()
}
