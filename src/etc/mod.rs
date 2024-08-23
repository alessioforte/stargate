pub mod cfg;
// pub mod config;
pub mod gate;
pub mod logo;
pub mod state;
pub mod tls;
// pub mod trie;

pub type AppData = actix_web::web::Data<state::State>;
pub type Gate = actix_web::web::Data<gate::Gate>;
