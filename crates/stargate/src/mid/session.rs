use actix_session::{storage::CookieSessionStore, SessionMiddleware};

pub fn cookie_session(secret_key: actix_web::cookie::Key) -> SessionMiddleware<CookieSessionStore> {
    SessionMiddleware::builder(CookieSessionStore::default(), secret_key).build()
}
