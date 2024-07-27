pub mod config;
pub mod state;
pub mod trie;

pub type AppData = actix_web::web::Data<state::State>;
