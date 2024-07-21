pub mod config;
pub mod globals;
pub mod trie;

use actix_web::web;

pub type AppData = web::Data<globals::Globals>;
